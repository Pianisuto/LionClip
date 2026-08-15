use std::{cell::RefCell, rc::Rc};

mod service;
mod suppression;

pub use service::{ClipboardService, ClipboardWriter};

pub type HistoryChangedCallback = Rc<RefCell<Option<Box<dyn Fn()>>>>;
