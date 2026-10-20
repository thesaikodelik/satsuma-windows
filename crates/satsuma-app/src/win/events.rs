//! Events from windows to the controller. Windows post here instead of calling the
//! controller directly, because they run inside OLE drag loops and window procedures.

use satsuma_core::{WheelItem, WheelMode};
use satsuma_engine::Operation;
use std::cell::RefCell;
use std::path::PathBuf;

/// Screen point in pixels.
pub type Point = (i32, i32);

#[derive(Debug)]
pub enum UiEvent {
    /// The invisible probe wheel received a drag carrying files: it is a real file drag.
    ProbeConfirmed { files: Vec<PathBuf> },
    /// The probe received a drag without files.
    ProbeRejected,
    WheelDrop { item: WheelItem, files: Vec<PathBuf>, at: Point },
    WheelPick { item: WheelItem },
    WheelCancel,
    WheelSwitch(WheelMode),
    HudClose,
    WindowClosed { window: isize },
    EditorApply { op: Operation, files: Vec<PathBuf>, at: Point },
    WindowDeactivated { window: isize },
    /// The Settings window changed the settings (already applied to the look; the app saves them).
    SettingsEdited(Box<satsuma_core::settings::Settings>),
}

thread_local! {
    static QUEUE: RefCell<Vec<UiEvent>> = const { RefCell::new(Vec::new()) };
}

pub fn post(event: UiEvent) {
    QUEUE.with(|q| q.borrow_mut().push(event));
}

pub fn drain() -> Vec<UiEvent> {
    QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()))
}
