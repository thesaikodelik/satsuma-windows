//! The macOS implementations of the platform hooks used by `cli.rs` and `lib.rs`.

use satsuma_core::{WheelItem, WheelMode};
use std::path::{Path, PathBuf};

pub use crate::app::run;

/// The macOS binary already has a terminal when one started it.
pub fn attach_console() {}

pub fn render_wheel(mode: WheelMode, items: Vec<WheelItem>, selected: Option<usize>, dark: bool, out: &Path) -> Result<(), String> {
    let mut model = crate::wheel::WheelModel::new(mode, items);
    model.selected = selected;
    let mtm = crate::system::main_thread();
    // AppKit drawing needs the shared application.
    let _app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    crate::wheel::render_png(mtm, model, dark, out)
}

pub fn render_icon(size: usize, out: &Path) -> Result<(), String> {
    let mtm = crate::system::main_thread();
    let _app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    crate::icon::render_icon(size, out)
}

pub fn render_editor(kind: &str, files: Vec<PathBuf>, sets: &[(String, f64)], segment: Option<isize>, out: &Path) -> Result<(), String> {
    let mtm = crate::system::main_thread();
    let _app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let delegate = crate::app::AppDelegate::new(mtm);
    let d = objc2::runtime::ProtocolObject::from_ref(&*delegate);
    let editor = match kind {
        "crop" => crate::editor::EditorWindow::crop(mtm, files, d, false),
        "edit" => crate::editor::EditorWindow::adjust(mtm, files, d, false),
        _ => crate::editor::EditorWindow::frame(mtm, files, d, false),
    };
    let editor = editor.inspect(|e| {
        for (k, v) in sets {
            e.set_adjustment(k, *v);
        }
    });
    editor.and_then(|e| e.render_png(segment, out))
}

pub fn ffmpeg_hint() -> &'static str {
    "Install ffmpeg (`brew install ffmpeg`) for audio and video."
}
