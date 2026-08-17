use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

use crate::{
    history::{ImageData, TextHistoryItem},
    image_store,
    storage::StoragePaths,
};

/// Coordinates filesystem cleanup with asynchronous image capture.
///
/// History mutations happen on the GTK main thread while image decoding/blob
/// publication runs in blocking workers. A delete must therefore not unlink a
/// content-addressed blob while another capture is still deciding whether to
/// reuse that same hash. Cleanup is queued while any image capture is in flight
/// and is flushed synchronously once the last capture finishes. The flush only
/// performs unlink metadata operations; it never reads or decodes image data.
#[derive(Clone)]
pub struct ImageCleanupCoordinator {
    inner: Rc<CleanupState>,
}

struct CleanupState {
    paths: StoragePaths,
    in_flight: Cell<usize>,
    pending: RefCell<Vec<ImageData>>,
}

impl ImageCleanupCoordinator {
    pub fn new(paths: StoragePaths) -> Self {
        Self {
            inner: Rc::new(CleanupState {
                paths,
                in_flight: Cell::new(0),
                pending: RefCell::new(Vec::new()),
            }),
        }
    }

    pub fn begin_capture(&self) {
        self.inner
            .in_flight
            .set(self.inner.in_flight.get().saturating_add(1));
    }

    pub fn finish_capture(&self, items: &[TextHistoryItem]) {
        let current = self.inner.in_flight.get();
        if current == 0 {
            eprintln!("lionclip: image cleanup invariant failed stage=capture-finish");
            return;
        }
        self.inner.in_flight.set(current - 1);
        self.flush(items);
    }

    pub fn queue(&self, image: ImageData) {
        let hash = image.content_hash();
        let mut pending = self.inner.pending.borrow_mut();
        if pending
            .iter()
            .any(|candidate| candidate.content_hash() == hash)
        {
            return;
        }
        pending.push(image);
    }

    pub fn flush(&self, items: &[TextHistoryItem]) {
        if self.inner.in_flight.get() != 0 {
            return;
        }
        // Every history mutation flushes, but almost none of them queue
        // anything: without this the common path still walked the whole
        // history to build a reference set it then never consulted.
        if self.inner.pending.borrow().is_empty() {
            return;
        }

        let referenced: HashSet<&str> = items
            .iter()
            .filter_map(TextHistoryItem::image)
            .map(ImageData::content_hash)
            .collect();
        let pending = self.inner.pending.take();

        for image in pending {
            if !referenced.contains(image.content_hash()) {
                image_store::delete_asset(&self.inner.paths, &image);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::history::{ImageMime, TextHistory};

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestStorage {
        root: PathBuf,
        paths: StoragePaths,
    }

    impl TestStorage {
        fn new() -> Self {
            let suffix = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "lionclip-image-cleanup-{}-{suffix}",
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

    fn image() -> ImageData {
        ImageData::new("a".repeat(64), ImageMime::Png, 3, 1, 1)
    }

    fn create_asset(paths: &StoragePaths, image: &ImageData) {
        fs::create_dir_all(paths.blobs()).unwrap();
        fs::create_dir_all(paths.thumbnails()).unwrap();
        fs::write(image_store::blob_path(paths, image).unwrap(), b"png").unwrap();
        fs::write(image_store::thumbnail_path(paths, image).unwrap(), b"thumb").unwrap();
    }

    #[test]
    fn cleanup_waits_for_active_capture_and_preserves_readded_hash() {
        let storage = TestStorage::new();
        let image = image();
        create_asset(&storage.paths, &image);
        let blob = image_store::blob_path(&storage.paths, &image).unwrap();
        let thumbnail = image_store::thumbnail_path(&storage.paths, &image).unwrap();
        let cleanup = ImageCleanupCoordinator::new(storage.paths.clone());

        cleanup.begin_capture();
        cleanup.queue(image.clone());
        cleanup.flush(&[]);
        assert!(blob.is_file());
        assert!(thumbnail.is_file());

        let mut history = TextHistory::default();
        history.record_image(image.clone());
        cleanup.finish_capture(history.items());
        assert!(blob.is_file());
        assert!(thumbnail.is_file());

        cleanup.queue(image);
        cleanup.flush(&[]);
        assert!(!blob.exists());
        assert!(!thumbnail.exists());
    }
}
