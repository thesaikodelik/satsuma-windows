//! The Float shelf (placeholder until the shelf window lands).

use super::events::Point;
use std::path::PathBuf;

pub struct FloatWindow;

impl FloatWindow {
    pub fn new(_files: &[PathBuf], _at: Point) -> FloatWindow {
        FloatWindow
    }
    pub fn id(&self) -> isize {
        0
    }
    pub fn add(&self, _files: &[PathBuf]) {}
    pub fn bring_to_front(&self) {}
}
