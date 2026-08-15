use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use rusqlite::{Connection, params};

use super::{HistoryItemId, TextHistoryItem};

const SCHEMA_VERSION: i64 = 1;

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
    pub(super) fn open(path: PathBuf) -> Result<(Self, Vec<TextHistoryItem>), PersistenceError> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (initial_sender, initial_receiver) = mpsc::sync_channel(1);
        let join_handle = thread::Builder::new()
            .name("lionclip-database".into())
            .spawn(move || worker_main(path, command_receiver, initial_sender))
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
    path: PathBuf,
    command_receiver: mpsc::Receiver<WorkerCommand>,
    initial_sender: mpsc::SyncSender<Result<Vec<TextHistoryItem>, PersistenceError>>,
) {
    let mut repository = match Repository::open(&path) {
        Ok(repository) => repository,
        Err(error) => {
            let _ = initial_sender.send(Err(error));
            return;
        }
    };
    let initial_items = match repository.load() {
        Ok(items) => items,
        Err(error) => {
            let _ = initial_sender.send(Err(error));
            return;
        }
    };
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
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| PersistenceError::at("migration-version-read"))?;

        if version > SCHEMA_VERSION {
            return Err(PersistenceError::at("migration-version-newer"));
        }

        if version == 0 {
            let transaction = self
                .connection
                .transaction()
                .map_err(|_| PersistenceError::at("migration-transaction"))?;
            transaction
                .execute_batch(
                    "CREATE TABLE history_items (
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
                )
                .map_err(|_| PersistenceError::at("migration-schema-v1"))?;
            transaction
                .pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(|_| PersistenceError::at("migration-version-write"))?;
            transaction
                .commit()
                .map_err(|_| PersistenceError::at("migration-commit"))?;
        }

        Ok(())
    }

    fn load(&self) -> Result<Vec<TextHistoryItem>, PersistenceError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, text_content, created_sequence, last_used_sequence, pinned
                 FROM history_items
                 WHERE kind = 'text'
                 ORDER BY last_used_sequence DESC, id DESC",
            )
            .map_err(|_| PersistenceError::at("history-load-prepare"))?;
        let rows = statement
            .query_map([], |row| {
                Ok(TextHistoryItem::new(
                    HistoryItemId::new(row.get(0)?),
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get::<_, i64>(4)? != 0,
                ))
            })
            .map_err(|_| PersistenceError::at("history-load-query"))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|_| PersistenceError::at("history-load-row"))
    }

    fn apply(&mut self, mutation: PersistenceMutation) -> Result<(), PersistenceError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|_| PersistenceError::at("history-write-transaction"))?;

        let removed_ids = match mutation {
            PersistenceMutation::Upsert { item, removed_ids } => {
                transaction
                    .execute(
                        "INSERT INTO history_items (
                            id, kind, text_content, created_sequence, last_used_sequence, pinned
                         ) VALUES (?1, 'text', ?2, ?3, ?4, ?5)
                         ON CONFLICT(id) DO UPDATE SET
                            text_content = excluded.text_content,
                            last_used_sequence = excluded.last_used_sequence,
                            pinned = excluded.pinned",
                        params![
                            item.id().value(),
                            item.text(),
                            item.created_sequence(),
                            item.last_used_sequence(),
                            item.is_pinned()
                        ],
                    )
                    .map_err(|_| PersistenceError::at("history-upsert"))?;
                removed_ids
            }
            PersistenceMutation::Delete { removed_ids } => removed_ids,
        };

        for id in removed_ids {
            transaction
                .execute("DELETE FROM history_items WHERE id = ?1", [id.value()])
                .map_err(|_| PersistenceError::at("history-retention-delete"))?;
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

    #[cfg(test)]
    fn item_count(&self) -> Result<usize, PersistenceError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM history_items", [], |row| row.get(0))
            .map_err(|_| PersistenceError::at("history-count"))?;
        usize::try_from(count).map_err(|_| PersistenceError::at("history-count"))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDatabase {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TestDatabase {
        fn new(test_name: &str) -> Self {
            let suffix = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "lionclip-{test_name}-{}-{suffix}",
                std::process::id()
            ));
            let path = directory.join("lionclip.db");
            Self { directory, path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn empty_database_migrates_to_v1_and_reopens_idempotently() {
        let database = TestDatabase::new("migration");
        let repository = Repository::open(database.path()).unwrap();
        assert_eq!(repository.schema_version(), Ok(SCHEMA_VERSION));
        assert_eq!(
            fs::metadata(&database.directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(database.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(repository);

        let reopened = Repository::open(database.path()).unwrap();
        assert_eq!(reopened.schema_version(), Ok(SCHEMA_VERSION));
        assert_eq!(reopened.item_count(), Ok(0));
    }

    #[test]
    fn repository_round_trips_exact_text_and_metadata() {
        let database = TestDatabase::new("exact-text");
        let mut repository = Repository::open(database.path()).unwrap();
        let exact = "  leading  \n\n\tUnicode: Olá 🦁\r\ntrailing  \n";
        let item = TextHistoryItem::new(HistoryItemId::new(7), exact.into(), 3, 9, true);
        repository
            .apply(PersistenceMutation::Upsert {
                item: item.clone(),
                removed_ids: Vec::new(),
            })
            .unwrap();
        drop(repository);

        let reopened = Repository::open(database.path()).unwrap();
        assert_eq!(reopened.load(), Ok(vec![item]));
    }
}
