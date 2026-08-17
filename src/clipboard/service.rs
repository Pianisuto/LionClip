use std::{
    cell::{Cell, RefCell},
    fs,
    rc::Rc,
};

use gtk::{gdk, gio, gio::prelude::InputStreamExtManual, glib, prelude::ObjectExt};

use crate::{
    history::{HistoryUpdate, ImageData, ImageMime, TextHistory},
    image_cleanup::ImageCleanupCoordinator,
    image_store::{self, MAX_IMAGE_ENCODED_BYTES},
    settings::SettingsService,
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
    /// Restores `text` to the clipboard. Always succeeds: `gdk::Clipboard`'s
    /// text setter has no failure signal to report. Returns `bool` (always
    /// `true`) rather than `()` so callers deciding whether to follow a
    /// restore with an auto-paste attempt do not need a separate rule for
    /// text versus images.
    pub fn restore_text(&self, text: &str) -> bool {
        self.suppression.borrow_mut().arm_text(text);
        self.clipboard.set_text(text);
        true
    }

    /// Drops a self-write suppression armed by a restore that happened while
    /// recording was paused: a paused clipboard handler returns before ever
    /// consulting it, so it would otherwise stay armed and could wrongly
    /// suppress the next real external copy after recording resumes.
    pub fn cancel_pending_self_write(&self) {
        self.suppression.borrow_mut().cancel();
    }

    /// Restores `image` to the clipboard, returning whether it actually
    /// succeeded. An auto-paste attempt must never follow a restore that
    /// silently failed.
    pub fn restore_image(&self, image: &ImageData) -> bool {
        let Some(path) = image_store::blob_path(&self.paths, image) else {
            eprintln!("lionclip: image restore failed stage=invalid-blob-key");
            return false;
        };

        // Restoration must complete before the popup closes: otherwise a fast
        // Ctrl+V can still see the previous clipboard while an async file read is
        // pending. This reads only the bounded compressed blob (max 25 MiB); it
        // never decodes full-resolution pixels on the GTK thread.
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(_) => {
                eprintln!("lionclip: image restore failed stage=blob-read");
                return false;
            }
        };
        let bytes = glib::Bytes::from_owned(bytes);
        let provider = gdk::ContentProvider::for_bytes(image.mime_type().as_str(), &bytes);
        self.suppression
            .borrow_mut()
            .arm_image(image.content_hash());
        if self.clipboard.set_content(Some(&provider)).is_err() {
            self.suppression.borrow_mut().cancel();
            eprintln!("lionclip: image restore failed stage=clipboard-set");
            return false;
        }
        true
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
        image_cleanup: ImageCleanupCoordinator,
        settings: Rc<SettingsService>,
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
            let image_cleanup = image_cleanup.clone();
            let settings = settings.clone();

            move |clipboard| {
                // Paused recording does no work at all, not even inspecting
                // the offered formats: nothing here may read or decode a
                // payload while the user asked LionClip to stop capturing.
                if settings.recording_paused() {
                    return;
                }

                let sequence = change_sequence.get().wrapping_add(1);
                change_sequence.set(sequence);
                let generation = history.borrow().generation();

                if settings.save_images()
                    && let Some(mime_type) = preferred_image_mime(clipboard)
                {
                    capture_image(
                        clipboard.clone(),
                        mime_type,
                        sequence,
                        generation,
                        change_sequence.clone(),
                        history.clone(),
                        history_changed.clone(),
                        suppression.clone(),
                        paths.clone(),
                        image_cleanup.clone(),
                        settings.clone(),
                    );
                } else {
                    // Also the path taken when an image is offered but
                    // `save_images` is off: it reads whatever plain-text
                    // representation the clipboard offers alongside the
                    // image, rather than discarding useful text just
                    // because an ignored image was offered too.
                    capture_text(
                        clipboard.clone(),
                        sequence,
                        generation,
                        change_sequence.clone(),
                        history.clone(),
                        history_changed.clone(),
                        suppression.clone(),
                        settings.clone(),
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

#[allow(clippy::too_many_arguments)]
fn capture_text(
    clipboard: gdk::Clipboard,
    sequence: u64,
    generation: u64,
    change_sequence: Rc<Cell<u64>>,
    history: Rc<RefCell<TextHistory>>,
    history_changed: HistoryChangedCallback,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
    settings: Rc<SettingsService>,
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
        // A bulk clear (Preferences "Clear history", or the popup's "Clear
        // unpinned") that ran while this read was in flight must not have a
        // just-cleared item reappear because a stale capture finishes late.
        if history.borrow().generation() != generation {
            return;
        }
        // Re-checked after the asynchronous read rather than only when the
        // clipboard changed: recording may have been paused while this was
        // in flight, and "stop capturing new items" has to cover the ones
        // whose reading merely started earlier.
        if settings.recording_paused() {
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
    generation: u64,
    change_sequence: Rc<Cell<u64>>,
    history: Rc<RefCell<TextHistory>>,
    history_changed: HistoryChangedCallback,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
    paths: StoragePaths,
    image_cleanup: ImageCleanupCoordinator,
    settings: Rc<SettingsService>,
) {
    // Mark the whole asynchronous capture, including blob publication and the
    // history decision, as in-flight. History cleanup cannot unlink an image
    // blob until every capture has settled.
    image_cleanup.begin_capture();
    let finish_history = history.clone();
    let finish_cleanup = image_cleanup.clone();

    glib::MainContext::default().spawn_local(async move {
        capture_image_task(
            clipboard,
            requested_mime,
            sequence,
            generation,
            change_sequence,
            history,
            history_changed,
            suppression,
            paths,
            image_cleanup,
            settings,
        )
        .await;

        finish_cleanup.finish_capture(finish_history.borrow().items());
    });
}

#[allow(clippy::too_many_arguments)]
async fn capture_image_task(
    clipboard: gdk::Clipboard,
    requested_mime: ImageMime,
    sequence: u64,
    generation: u64,
    change_sequence: Rc<Cell<u64>>,
    history: Rc<RefCell<TextHistory>>,
    history_changed: HistoryChangedCallback,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
    paths: StoragePaths,
    image_cleanup: ImageCleanupCoordinator,
    settings: Rc<SettingsService>,
) {
    let (stream, negotiated_mime) = match clipboard
        .read_future(&[requested_mime.as_str()], glib::Priority::DEFAULT)
        .await
    {
        Ok(value) => value,
        Err(_) => {
            fallback_to_text(
                clipboard,
                sequence,
                generation,
                change_sequence,
                history,
                history_changed,
                suppression,
                settings.clone(),
            );
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
            fallback_to_text(
                clipboard,
                sequence,
                generation,
                change_sequence,
                history,
                history_changed,
                suppression,
                settings.clone(),
            );
            return;
        }
    };
    if change_sequence.get() != sequence {
        return;
    }
    if partial_error.is_some() || read == 0 || read > MAX_IMAGE_ENCODED_BYTES {
        eprintln!("lionclip: image capture rejected reason=encoded-size-or-read");
        fallback_to_text(
            clipboard,
            sequence,
            generation,
            change_sequence,
            history,
            history_changed,
            suppression,
            settings.clone(),
        );
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
            eprintln!(
                "lionclip: image capture rejected reason={}",
                error.diagnostic()
            );
            fallback_to_text(
                clipboard,
                sequence,
                generation,
                change_sequence,
                history,
                history_changed,
                suppression,
                settings.clone(),
            );
            return;
        }
        Err(_) => {
            eprintln!("lionclip: image capture rejected reason=worker-panic");
            fallback_to_text(
                clipboard,
                sequence,
                generation,
                change_sequence,
                history,
                history_changed,
                suppression,
                settings.clone(),
            );
            return;
        }
    };

    if change_sequence.get() != sequence {
        image_cleanup.queue(stored.image);
        return;
    }
    if suppression
        .borrow_mut()
        .should_suppress_image(stored.image.content_hash())
    {
        return;
    }

    // Both guards are re-evaluated here rather than only when the clipboard
    // changed. This capture spent real time decoding and writing its blob,
    // during which the user may have paused recording, turned off image
    // capture, or cleared the history; none of those should be undone by an
    // item that merely started arriving earlier. A rejected image hands its
    // freshly written blob to the cleanup coordinator below.
    let policy_allows = !settings.recording_paused() && settings.save_images();
    let update = if policy_allows && history.borrow().generation() == generation {
        history.borrow_mut().record_image(stored.image.clone())
    } else {
        HistoryUpdate::Rejected
    };
    if matches!(update, HistoryUpdate::Rejected) {
        image_cleanup.queue(stored.image);
    }
    notify_if_changed(update, &history_changed);
}

#[allow(clippy::too_many_arguments)]
fn fallback_to_text(
    clipboard: gdk::Clipboard,
    sequence: u64,
    generation: u64,
    change_sequence: Rc<Cell<u64>>,
    history: Rc<RefCell<TextHistory>>,
    history_changed: HistoryChangedCallback,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
    settings: Rc<SettingsService>,
) {
    if change_sequence.get() != sequence {
        return;
    }
    capture_text(
        clipboard,
        sequence,
        generation,
        change_sequence,
        history,
        history_changed,
        suppression,
        settings,
    );
}

fn notify_if_changed(update: HistoryUpdate, history_changed: &HistoryChangedCallback) {
    if update.changed()
        && let Some(callback) = history_changed.borrow().as_ref()
    {
        callback();
    }
}
