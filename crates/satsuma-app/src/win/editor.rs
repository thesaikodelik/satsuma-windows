//! The editor windows (placeholder until they land).

use std::path::PathBuf;
use windows::Win32::UI::WindowsAndMessaging::MSG;

pub struct EditorWindow;

impl EditorWindow {
    pub fn open(_tool: satsuma_core::Tool, _files: Vec<PathBuf>) -> Result<EditorWindow, String> {
        Err("Not built yet.".into())
    }
    pub fn id(&self) -> isize {
        0
    }
}

pub fn translate_dialog_message(_msg: &MSG) -> bool {
    false
}
