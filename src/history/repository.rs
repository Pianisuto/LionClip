use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
    sync::mpsc,
    thread,
    time::Duration,
};

use rusqlite::{Connection, params};

use crate::{image_store, storage::StoragePaths};

use super::{HistoryItemId, ImageData, ImageMime, TextHistoryItem};

const SCHEMA_VERSION: i64 = 2;

struct Migration {
    version: i64,
    sql: &'static str,
    error_stage: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: "CREATE TABLE history_items (
                id INTEGER PRIMARY KEY CHECK (id >= 0),
                kind TEXT NOT NULL CHECK (kind = 'text'),
                text_content TEXT NOT NULL,
                created_sequence INTEGER NOT NULL CHECK (created_sequence >= 0),
                last_used_sequence INTEGER NOT NULL UNIQUE CHECK (last_used_sequence >= 0),
                pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
                UNIQUE (kind, text_content)
            );
            CREATE INDEX history_items_order
                ON history_items(last_used_sequence DESC);",
        error_stage: "migration-schema-v1",
    },
    Migration {
        version: 2,
        sql: "ALTER TABLE history_items RENAME TO history_items_v1;
            DROP INDEX IF EXISTS history_items_order;

            CREATE TABLE history_items (
                id INTEGER PRIMARY KEY CHECK (id >= 0),
                kind TEXT NOT NULL CHECK (kind IN ('text', 'image')),
                text_content TEXT,
                content_hash TEXT,
                mime_type TEXT,
                byte_length INTEGER,
                image_width INTEGER,
                image_height INTEGER,
                created_sequence INTEGER NOT NULL CHECK (created_sequence >= 0),
                last_used_sequence INTEGER NOT NULL UNIQUE CHECK (last_used_sequence >= 0),
                pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
                CHECK (
                    (kind = 'text'
                        AND text_content IS NOT NULL
                        AND content_hash IS NULL
                        AND mime_type IS NULL
                        AND byte_length IS NULL
                        AND image_width IS NULL
                        AND image_height IS NULL)
                    OR
                    (kind = 'image'
                        AND text_content IS NULL
                        AND content_hash IS NOT NULL
                        AND length(content_hash) = 64
                        AND mime_type IN ('image/png', 'image/jpeg')
                        AND byte_length > 0
                        AND image_width > 0
                        AND image_height > 0)
                )
            );

            INSERT INTO history_items (
                id, kind, text_content, created_sequence, last_used_sequence, pinned
            )
            SELECT id, 'text', text_content, created_sequence, last_used_sequence, pinned
            FROM history_items_v1;

            DROP TABLE history_items_v1;

            CREATE UNIQUE INDEX history_text_unique
                ON history_items(text_content) WHERE kind = 'text';
            CREATE UNIQUE INDEX history_image_hash_unique
                ON history_items(content_hash) WHERE kind = 'image';
            CREATE INDEX history_items_order
                ON history_items(last_used_sequence DESC);",
        error_stage: "migration-schema-v2",
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PersistenceError {
    stage: &'static str,
}

impl PersistenceError {
    fn at(stage: &'static str) -> Self {
        Self { stage }
    }

    pub(crate) fn diagnostic(self) -> &'static str {
        self.stage
    }
}

#[derive(Debug)]
pub(super) enum PersistenceMutation {
    Upsert {
        item: TextHistoryItem,
        removed_ids: Vec<HistoryItemId>,
    },
    Delete {
        removed_ids: Vec<HistoryItemId>,
    },
    ClearUnpinned,
    ClearAll,
}

enum WorkerCommand {
    Apply(PersistenceMutation),
    Shutdown(mpsc::SyncSender<Result<(), PersistenceError>>),
}

pub(super) struct PersistenceWorker {
    sender: Option<mpsc::Sender<WorkerCommand>>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl PersistenceWorker {
    pub(super) fn open(
        paths: StoragePaths,
    ) -> Result<(Self, Vec<TextHistoryItem>), PersistenceError> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (initial_sender, initial_receiver) = mpsc::sync_channel(1);
        let join_handle = thread::Builder::new()
            .name("lionclip-database".into())
            .spawn(move || worker_main(paths, command_receiver, initial_sender))
            .map_err(|_| PersistenceError::at("worker-start"))?;

        let initial_items = match initial_receiver.recv() {
            Ok(Ok(items)) => items,
            Ok(Err(error)) => {
                let _ = join_handle.join();
                return Err(error);
            }
            Err(_) => {
                let _ = join_handle.join();
                return Err(PersistenceError::at("worker-initialization"));
            }
        };

        Ok((
            Self {
                sender: Some(command_sender),
                join_handle: Some(join_handle),
            },
            initial_items,
        ))
    }

    pub(super) fn submit(&self, mutation: PersistenceMutation) {
        let sent = self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send(WorkerCommand::Apply(mutation)).is_ok());
        if !sent {
            eprintln!("lionclip: persistence unavailable stage=worker-queue");
        }
    }

    fn shutdown(&mut self) {
        let Some(sender) = self.sender.take() else {
            return;
        };
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        if sender.send(WorkerCommand::Shutdown(result_sender)).is_err() {
            eprintln!("lionclip: persistence unavailable stage=worker-shutdown");
        } else {
            match result_receiver.recv() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!(
                    "lionclip: persistence unavailable stage={}",
                    error.diagnostic()
                ),
                Err(_) => {
                    eprintln!("lionclip: persistence unavailable stage=worker-shutdown-result");
                }
            }
        }
        if self
            .join_handle
            .take()
            .is_some_and(|handle| handle.join().is_err())
        {
            eprintln!("lionclip: persistence unavailable stage=worker-join");
        }
    }
}

impl Drop for PersistenceWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_main(
    paths: StoragePaths,
    command_receiver: mpsc::Receiver<WorkerCommand>,
    initial_sender: mpsc::SyncSender<Result<Vec<TextHistoryItem>, PersistenceError>>,
) {
    let mut repository = match Repository::open(paths.database()) {
        Ok(repository) => repository,
        Err(error) => {
            let _ = initial_sender.send(Err(error));
            return;
        }
    };
    let mut initial_items = match repository.load() {
        Ok(items) => items,
        Err(error) => {
            let _ = initial_sender.send(Err(error));
            return;
        }
    };

    // Startup is the one filesystem reconciliation owned by the database
    // worker: no live clipboard capture exists yet, so there is no concurrent
    // blob publication to race with orphan cleanup.
    match image_store::reconcile(&paths, &initial_items) {
        Ok(missing_ids) if !missing_ids.is_empty() => {
            if let Err(error) = repository.apply(PersistenceMutation::Delete {
                removed_ids: missing_ids.clone(),
            }) {
                let _ = initial_sender.send(Err(error));
                return;
            }
            initial_items.retain(|item| !missing_ids.contains(&item.id()));
        }
        Ok(_) => {}
        Err(error) => eprintln!(
            "lionclip: image storage reconciliation unavailable stage={}",
            error.diagnostic()
        ),
    }

    if initial_sender.send(Ok(initial_items)).is_err() {
        return;
    }

    let mut write_failure = None;
    while let Ok(command) = command_receiver.recv() {
        match command {
            WorkerCommand::Apply(mutation) => {
                if let Err(error) = repository.apply(mutation) {
                    write_failure = Some(error);
                    eprintln!(
                        "lionclip: persistence write failed stage={}",
                        error.diagnostic()
                    );
                }
            }
            WorkerCommand::Shutdown(result_sender) => {
                let _ = result_sender.send(write_failure.map_or(Ok(()), Err));
                return;
            }
        }
    }
}

struct Repository {
    connection: Connection,
}

impl Repository {
    fn open(path: &Path) -> Result<Self, PersistenceError> {
        let parent = path
            .parent()
            .ok_or_else(|| PersistenceError::at("data-directory"))?;
        let mut directory_builder = fs::DirBuilder::new();
        directory_builder.recursive(true).mode(0o700);
        directory_builder
            .create(parent)
            .map_err(|_| PersistenceError::at("data-directory"))?;

        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|_| PersistenceError::at("database-file"))?;

        let connection =
            Connection::open(path).map_err(|_| PersistenceError::at("database-open"))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| PersistenceError::at("database-configuration"))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|_| PersistenceError::at("database-configuration"))?;

        let mut repository = Self { connection };
        repository.migrate()?;
        Ok(repository)
    }

    fn migrate(&mut self) -> Result<(), PersistenceError> {
        let mut version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| PersistenceError::at("migration-version-read"))?;
        if version > SCHEMA_VERSION {
            return Err(PersistenceError::at("migration-version-newer"));
        }

        for migration in MIGRATIONS {
            if migration.version <= version {
                continue;
            }
            if migration.version != version + 1 {
                return Err(PersistenceError::at("migration-sequence"));
            }
            let transaction = self
                .connection
                .transaction()
                .map_err(|_| PersistenceError::at("migration-transaction"))?;
            transaction
                .execute_batch(migration.sql)
                .map_err(|_| PersistenceError::at(migration.error_stage))?;
            transaction
                .pragma_update(None, "user_version", migration.version)
                .map_err(|_| PersistenceError::at("migration-version-write"))?;
            transaction
                .commit()
                .map_err(|_| PersistenceError::at("migration-commit"))?;
            version = migration.version;
        }
        Ok(())
    }

    fn load(&self) -> Result<Vec<TextHistoryItem>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, kind, text_content, content_hash, mime_type,
                        byte_length, image_width, image_height,
                        created_sequence, last_used_sequence, pinned
                 FROM history_items
                 ORDER BY last_used_sequence DESC, id DESC",
            )
            .map_err(|_| PersistenceError::at("history-load-prepare"))?;
        let rows = statement
            .query_map([], row_to_item)
            .map_err(|_| PersistenceError::at("history-load-query"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| PersistenceError::at("history-load-row"))
    }

    fn apply(&mut self, mutation: PersistenceMutation) -> Result<(), PersistenceError> {
        // This worker persists metadata only. Live blob deletion is coordinated
        // by the main-thread history/capture lifecycle so an older delete cannot
        // race a same-image recapture that is reusing the content-addressed file.
        let transaction = self
            .connection
            .transaction()
            .map_err(|_| PersistenceError::at("history-write-transaction"))?;
        match mutation {
            PersistenceMutation::Upsert { item, removed_ids } => {
                upsert_item(&transaction, &item)?;
                delete_ids(&transaction, &removed_ids, "history-retention-delete")?;
            }
            PersistenceMutation::Delete { removed_ids } => {
                delete_ids(&transaction, &removed_ids, "history-delete")?;
            }
            PersistenceMutation::ClearUnpinned => {
                transaction
                    .execute("DELETE FROM history_items WHERE pinned = 0", [])
                    .map_err(|_| PersistenceError::at("history-clear-unpinned"))?;
            }
            PersistenceMutation::ClearAll => {
                transaction
                    .execute("DELETE FROM history_items", [])
                    .map_err(|_| PersistenceError::at("history-clear-all"))?;
            }
        }
        transaction
            .commit()
            .map_err(|_| PersistenceError::at("history-write-commit"))
    }

    #[cfg(test)]
    fn schema_version(&self) -> Result<i64, PersistenceError> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| PersistenceError::at("migration-version-read"))
    }
}

fn upsert_item(
    transaction: &rusqlite::Transaction<'_>,
    item: &TextHistoryItem,
) -> Result<(), PersistenceError> {
    if let Some(text) = item.as_text() {
        transaction
            .execute(
                "INSERT INTO history_items (
                    id, kind, text_content, content_hash, mime_type, byte_length,
                    image_width, image_height, created_sequence, last_used_sequence, pinned
                 ) VALUES (?1, 'text', ?2, NULL, NULL, NULL, NULL, NULL, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                    kind = 'text', text_content = excluded.text_content,
                    content_hash = NULL, mime_type = NULL, byte_length = NULL,
                    image_width = NULL, image_height = NULL,
                    last_used_sequence = excluded.last_used_sequence,
                    pinned = excluded.pinned",
                params![
                    item.id().value(),
                    text,
                    item.created_sequence(),
                    item.last_used_sequence(),
                    item.is_pinned()
                ],
            )
            .map_err(|_| PersistenceError::at("history-upsert-text"))?;
        return Ok(());
    }

    let image = item
        .image()
        .ok_or_else(|| PersistenceError::at("history-upsert-kind"))?;
    let byte_length = i64::try_from(image.byte_length())
        .map_err(|_| PersistenceError::at("history-upsert-image-size"))?;
    transaction
        .execute(
            "INSERT INTO history_items (
                id, kind, text_content, content_hash, mime_type, byte_length,
                image_width, image_height, created_sequence, last_used_sequence, pinned
             ) VALUES (?1, 'image', NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                kind = 'image', text_content = NULL,
                content_hash = excluded.content_hash,
                mime_type = excluded.mime_type,
                byte_length = excluded.byte_length,
                image_width = excluded.image_width,
                image_height = excluded.image_height,
                last_used_sequence = excluded.last_used_sequence,
                pinned = excluded.pinned",
            params![
                item.id().value(),
                image.content_hash(),
                image.mime_type().as_str(),
                byte_length,
                i64::from(image.width()),
                i64::from(image.height()),
                item.created_sequence(),
                item.last_used_sequence(),
                item.is_pinned()
            ],
        )
        .map_err(|_| PersistenceError::at("history-upsert-image"))?;
    Ok(())
}

fn delete_ids(
    transaction: &rusqlite::Transaction<'_>,
    ids: &[HistoryItemId],
    stage: &'static str,
) -> Result<(), PersistenceError> {
    for id in ids {
        transaction
            .execute("DELETE FROM history_items WHERE id = ?1", [id.value()])
            .map_err(|_| PersistenceError::at(stage))?;
    }
    Ok(())
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<TextHistoryItem> {
    let id = HistoryItemId::new(row.get(0)?);
    let kind: String = row.get(1)?;
    let created_sequence = row.get(8)?;
    let last_used_sequence = row.get(9)?;
    let pinned = row.get::<_, i64>(10)? != 0;
    match kind.as_str() {
        "text" => Ok(TextHistoryItem::new_text(
            id,
            row.get(2)?,
            created_sequence,
            last_used_sequence,
            pinned,
        )),
        "image" => Ok(TextHistoryItem::new_image(
            id,
            image_from_columns(row, 3)?,
            created_sequence,
            last_used_sequence,
            pinned,
        )),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn image_from_columns(row: &rusqlite::Row<'_>, start: usize) -> rusqlite::Result<ImageData> {
    let hash: String = row.get(start)?;
    let mime: String = row.get(start + 1)?;
    let mime = ImageMime::parse(&mime).ok_or(rusqlite::Error::InvalidQuery)?;
    let byte_length =
        u64::try_from(row.get::<_, i64>(start + 2)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let width =
        u32::try_from(row.get::<_, i64>(start + 3)?).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let height =
        u32::try_from(row.get::<_, i64>(start + 4)?).map_err(|_| rusqlite::Error::InvalidQuery)?;

    let dimensions_valid = width > 0
        && height > 0
        && width <= image_store::MAX_IMAGE_DIMENSION
        && height <= image_store::MAX_IMAGE_DIMENSION
        && u64::from(width).saturating_mul(u64::from(height)) <= image_store::MAX_IMAGE_PIXELS;
    if byte_length == 0
        || byte_length > image_store::MAX_IMAGE_ENCODED_BYTES as u64
        || !dimensions_valid
    {
        return Err(rusqlite::Error::InvalidQuery);
    }

    Ok(ImageData::new(hash, mime, byte_length, width, height))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct TestStorage {
        root: std::path::PathBuf,
        paths: StoragePaths,
    }

    impl TestStorage {
        fn new(name: &str) -> Self {
            let suffix = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "lionclip-repository-{name}-{}-{suffix}",
                std::process::id()
            ));
            let paths = StoragePaths::for_root(root.clone());
            Self { root, paths }
        }
    }

    impl Drop for TestStorage {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn fresh_database_reaches_schema_v2() {
        let storage = TestStorage::new("fresh-v2");
        let repository = Repository::open(storage.paths.database()).unwrap();
        assert_eq!(repository.schema_version(), Ok(2));
    }

    #[test]
    fn v1_text_rows_migrate_losslessly_to_v2() {
        let storage = TestStorage::new("migration-v1-v2");
        fs::create_dir_all(&storage.root).unwrap();
        let connection = Connection::open(storage.paths.database()).unwrap();
        connection.execute_batch(MIGRATIONS[0].sql).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        connection
            .execute(
                "INSERT INTO history_items
                 (id, kind, text_content, created_sequence, last_used_sequence, pinned)
                 VALUES (7, 'text', ?1, 3, 9, 1)",
                [" exact\ntext\t"],
            )
            .unwrap();
        drop(connection);

        let repository = Repository::open(storage.paths.database()).unwrap();
        let items = repository.load().unwrap();
        assert_eq!(repository.schema_version(), Ok(2));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id(), HistoryItemId::new(7));
        assert_eq!(items[0].as_text(), Some(" exact\ntext\t"));
        assert!(items[0].is_pinned());
        assert_eq!(items[0].last_used_sequence(), 9);
    }

    #[test]
    fn repository_round_trips_image_metadata() {
        let storage = TestStorage::new("image-roundtrip");
        let mut repository = Repository::open(storage.paths.database()).unwrap();
        let item = TextHistoryItem::new_image(
            HistoryItemId::new(2),
            ImageData::new("a".repeat(64), ImageMime::Png, 1234, 640, 480),
            2,
            2,
            true,
        );
        repository
            .apply(PersistenceMutation::Upsert {
                item: item.clone(),
                removed_ids: Vec::new(),
            })
            .unwrap();
        assert_eq!(repository.load().unwrap(), vec![item]);
    }

    #[test]
    fn invalid_persisted_image_metadata_is_rejected() {
        let storage = TestStorage::new("invalid-image-metadata");
        let repository = Repository::open(storage.paths.database()).unwrap();
        repository
            .connection
            .execute(
                "INSERT INTO history_items (
                    id, kind, content_hash, mime_type, byte_length,
                    image_width, image_height, created_sequence, last_used_sequence, pinned
                 ) VALUES (1, 'image', ?1, 'image/png', 1, ?2, 1, 1, 1, 0)",
                params![
                    "a".repeat(64),
                    i64::from(image_store::MAX_IMAGE_DIMENSION) + 1
                ],
            )
            .unwrap();
        assert!(repository.load().is_err());
    }

    #[test]
    fn newer_schema_is_rejected() {
        let storage = TestStorage::new("newer");
        drop(Repository::open(storage.paths.database()).unwrap());
        let connection = Connection::open(storage.paths.database()).unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        drop(connection);
        assert!(matches!(
            Repository::open(storage.paths.database()),
            Err(PersistenceError {
                stage: "migration-version-newer"
            })
        ));
    }
}
