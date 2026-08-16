use std::{
    cell::{Cell, RefCell},
    fs,
    rc::Rc,
};

use gtk::{
    gdk, gio, glib,
    gio::prelude::InputStreamExtManual,
    prelude::ObjectExt,
};

use crate::{
    history::{HistoryUpdate, ImageData, ImageMime, TextHistory},
    image_store::{self, MAX_IMAGE_ENCODED_BYTES},
    storage::StoragePaths,
};

use super::{HistoryChangedCallback, suppression::SelfWriteSuppression};

#[derive(Clone)]
pub struct ClipboardWriter {
    clipboard: gdk::Clipboard,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
    paths: StoragePaths,
}

impl ClipboardWriter {
    pub fn restore_text(&self, text: &str) {
        self.suppression.borrow_mut().arm_text(text);
        self.clipboard.set_text(text);
    }

    pub fn restore_image(&self, image: &ImageData) {
        let Some(path) = image_store::blob_path(&self.paths, image) else {
            eprintln!("lionclip: image restore failed stage=invalid-blob-key");
            return;
        };
        let clipboard = self.clipboard.clone();
        let suppression = self.suppression.clone();
        let content_hash = image.content_hash().to_owned();
        let mime_type = image.mime_type();

        glib::MainContext::default().spawn_local(async move {
            let bytes = match gio::spawn_blocking(move || fs::read(path)).await {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(_)) | Err(_) => {
                    eprintln!("lionclip: image restore failed stage=blob-read");
                    return;
                }
            };
            let bytes = glib::Bytes::from_owned(bytes);
            let provider = gdk::ContentProvider::for_bytes(mime_type.as_str(), &bytes);
            suppression.borrow_mut().arm_image(&content_hash);
            if clipboard.set_content(Some(&provider)).is_err() {
                suppression.borrow_mut().cancel();
                eprintln!("lionclip: image restore failed stage=clipboard-set");
            }
        });
    }
}

pub struct ClipboardService {
    clipboard: gdk::Clipboard,
    handler_id: Option<glib::SignalHandlerId>,
    writer: ClipboardWriter,
}

impl ClipboardService {
    pub fn start(
        clipboard: gdk::Clipboard,
        history: Rc<RefCell<TextHistory>>,
        history_changed: HistoryChangedCallback,
        paths: StoragePaths,
    ) -> Self {
        let suppression = Rc::new(RefCell::new(SelfWriteSuppression::default()));
        let writer = ClipboardWriter {
            clipboard: clipboard.clone(),
            suppression: suppression.clone(),
            paths: paths.clone(),
        };
        let change_sequence = Rc::new(Cell::new(0_u64));

        let handler_id = clipboard.connect_changed({
            let history = history.clone();
            let history_changed = history_changed.clone();
            let suppression = suppression.clone();
            let change_sequence = change_sequence.clone();
            let paths = paths.clone();

            move |clipboard| {
                let sequence = change_sequence.get().wrapping_add(1);
                change_sequence.set(sequence);

                if let Some(mime_type) = preferred_image_mime(clipboard) {
                    capture_image(
                        clipboard.clone(),
                        mime_type,
                        sequence,
                        change_sequence.clone(),
                        history.clone(),
                        history_changed.clone(),
                        suppression.clone(),
                        paths.clone(),
                    );
                } else {
                    capture_text(
                        clipboard.clone(),
                        sequence,
                        change_sequence.clone(),
                        history.clone(),
                        history_changed.clone(),
                        suppression.clone(),
                    );
                }
            }
        });

        Self {
            clipboard,
            handler_id: Some(handler_id),
            writer,
        }
    }

    pub fn writer(&self) -> ClipboardWriter {
        self.writer.clone()
    }
}

impl Drop for ClipboardService {
    fn drop(&mut self) {
        if let Some(handler_id) = self.handler_id.take() {
            self.clipboard.disconnect(handler_id);
        }
    }
}

fn preferred_image_mime(clipboard: &gdk::Clipboard) -> Option<ImageMime> {
    let formats = clipboard.formats();
    for mime in [ImageMime::Png, ImageMime::Jpeg] {
        if formats.contain_mime_type(mime.as_str()) {
            return Some(mime);
        }
    }

    // A screenshot may be offered as a GdkTexture rather than raw MIME bytes.
    // GDK can serialize known GTypes into standard image MIME types; requesting
    // PNG here keeps that path conservative without adding another capture API.
    let serializable = formats.clone().union_serialize_mime_types();
    [ImageMime::Png, ImageMime::Jpeg]
        .into_iter()
        .find(|mime| serializable.contain_mime_type(mime.as_str()))
}

fn capture_text(
    clipboard: gdk::Clipboard,
    sequence: u64,
    change_sequence: Rc<Cell<u64>>,
    history: Rc<RefCell<TextHistory>>,
    history_changed: HistoryChangedCallback,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
) {
    glib::MainContext::default().spawn_local(async move {
        let read_result = clipboard.read_text_future().await;
        if change_sequence.get() != sequence {
            return;
        }
        let text = match read_result {
            Ok(Some(text)) => text.to_string(),
            Ok(None) | Err(_) => {
                suppression.borrow_mut().cancel();
                return;
            }
        };
        if suppression.borrow_mut().should_suppress_text(&text) {
            return;
        }
        let update = history.borrow_mut().record(text);
        notify_if_changed(update, &history_changed);
    });
}

#[allow(clippy::too_many_arguments)]
fn capture_image(
    clipboard: gdk::Clipboard,
    requested_mime: ImageMime,
    sequence: u64,
    change_sequence: Rc<Cell<u64>>,
    history: Rc<RefCell<TextHistory>>,
    history_changed: HistoryChangedCallback,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
    paths: StoragePaths,
) {
    glib::MainContext::default().spawn_local(async move {
        let (stream, negotiated_mime) = match clipboard
            .read_future(&[requested_mime.as_str()], glib::Priority::DEFAULT)
            .await
        {
            Ok(value) => value,
            Err(_) => {
                suppression.borrow_mut().cancel();
                return;
            }
        };
        if change_sequence.get() != sequence {
            return;
        }
        let mime_type = ImageMime::SUPPORTED
            .into_iter()
            .find(|candidate| *candidate == negotiated_mime.as_str())
            .and_then(ImageMime::parse)
            .unwrap_or(requested_mime);

        let buffer = vec![0_u8; MAX_IMAGE_ENCODED_BYTES + 1];
        let (mut bytes, read, partial_error) = match stream
            .read_all_future(buffer, glib::Priority::DEFAULT)
            .await
        {
            Ok(value) => value,
            Err((_buffer, _error)) => {
                suppression.borrow_mut().cancel();
                return;
            }
        };
        if change_sequence.get() != sequence {
            return;
        }
        if partial_error.is_some() || read == 0 || read > MAX_IMAGE_ENCODED_BYTES {
            suppression.borrow_mut().cancel();
            eprintln!("lionclip: image capture rejected reason=encoded-size-or-read");
            return;
        }
        bytes.truncate(read);

        let worker_paths = paths.clone();
        let stored = match gio::spawn_blocking(move || {
            image_store::process_and_store(&worker_paths, mime_type, bytes)
        })
        .await
        {
            Ok(Ok(stored)) => stored,
            Ok(Err(error)) => {
                suppression.borrow_mut().cancel();
                eprintln!(
                    "lionclip: image capture rejected reason={}",
                    error.diagnostic()
                );
                return;
            }
            Err(_) => {
                suppression.borrow_mut().cancel();
                eprintln!("lionclip: image capture rejected reason=worker-panic");
                return;
            }
        };

        if change_sequence.get() != sequence {
            cleanup_new_stale_asset(&paths, &history, &stored);
            return;
        }
        if suppression
            .borrow_mut()
            .should_suppress_image(stored.image.content_hash())
        {
            return;
        }

        let update = history.borrow_mut().record_image(stored.image.clone());
        if matches!(update, HistoryUpdate::Rejected) {
            cleanup_new_stale_asset(&paths, &history, &stored);
        }
        notify_if_changed(update, &history_changed);
    });
}

fn cleanup_new_stale_asset(
    paths: &StoragePaths,
    history: &Rc<RefCell<TextHistory>>,
    stored: &image_store::StoredImage,
) {
    if !stored.original_created
        || history
            .borrow()
            .contains_image_hash(stored.image.content_hash())
    {
        return;
    }
    let paths = paths.clone();
    let image = stored.image.clone();
    drop(gio::spawn_blocking(move || {
        image_store::delete_asset(&paths, &image);
    }));
}

fn notify_if_changed(update: HistoryUpdate, history_changed: &HistoryChangedCallback) {
    if update.changed()
        && let Some(callback) = history_changed.borrow().as_ref()
    {
        callback();
    }
}
