use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk::{gdk, glib, prelude::ObjectExt};

use crate::history::{HistoryUpdate, TextHistory};

use super::{HistoryChangedCallback, suppression::SelfWriteSuppression};

#[derive(Clone)]
pub struct ClipboardWriter {
    clipboard: gdk::Clipboard,
    suppression: Rc<RefCell<SelfWriteSuppression>>,
}

impl ClipboardWriter {
    pub fn restore_text(&self, text: &str) {
        self.suppression.borrow_mut().arm(text);
        self.clipboard.set_text(text);
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
    ) -> Self {
        let suppression = Rc::new(RefCell::new(SelfWriteSuppression::default()));
        let writer = ClipboardWriter {
            clipboard: clipboard.clone(),
            suppression: suppression.clone(),
        };
        let change_sequence = Rc::new(Cell::new(0_u64));

        let handler_id = clipboard.connect_changed({
            let history = history.clone();
            let history_changed = history_changed.clone();
            let suppression = suppression.clone();
            let change_sequence = change_sequence.clone();

            move |clipboard| {
                let sequence = change_sequence.get().wrapping_add(1);
                change_sequence.set(sequence);

                let clipboard = clipboard.clone();
                let history = history.clone();
                let history_changed = history_changed.clone();
                let suppression = suppression.clone();
                let change_sequence = change_sequence.clone();

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

                    if suppression.borrow_mut().should_suppress(&text) {
                        return;
                    }

                    let update: HistoryUpdate = history.borrow_mut().record(text);
                    let changed = update.changed();
                    if changed && let Some(callback) = history_changed.borrow().as_ref() {
                        callback();
                    }
                });
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
