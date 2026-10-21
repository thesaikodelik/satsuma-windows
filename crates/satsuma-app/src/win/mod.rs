//! The Windows app: notification-area icon, drag probe and wheel, HUD, Float shelf and
//! editors, drawn with Direct2D and DirectWrite.

/// `SATSUMA_DEBUG=1` logs gestures and jobs to a debugger (DebugView), and
/// `SATSUMA_DEBUG_LOG=<file>` appends them to a file.
pub(crate) fn debug_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SATSUMA_DEBUG").is_some_and(|v| v != "0") || std::env::var_os("SATSUMA_DEBUG_LOG").is_some())
}

pub(crate) fn debug_line(line: &str) {
    unsafe { windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(&windows::core::HSTRING::from(line)) };
    if let Some(path) = std::env::var_os("SATSUMA_DEBUG_LOG") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::win::debug_enabled() {
            $crate::win::debug_line(&format!("satsuma: {}\n", format!($($arg)*)));
        }
    };
}

mod app;
mod dragsource;
mod droptarget;
mod editor;
mod estimates;
mod events;
mod float;
mod form;
mod forms;
mod gfx;
mod hud;
mod icon;
mod liquid;
mod system;
#[cfg(debug_assertions)]
mod selftest;
mod settings_window;
pub(crate) mod setup;
mod cropvideo;
mod annotate;
mod bleep;
mod pages;
mod redact;
mod redactvideo;
mod theme;
mod timeline;
mod update;
mod wheel;
mod window;

use satsuma_core::{WheelItem, WheelMode};
use std::path::{Path, PathBuf};

pub use app::run;
#[cfg(debug_assertions)]
pub use selftest::{selftest_click, selftest_drag, selftest_screenshot, selftest_tour};

/// Lets `satsuma.exe <command>` print to the terminal that started it.
pub fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

pub fn render_wheel(mode: WheelMode, items: Vec<WheelItem>, selected: Option<usize>, dark: bool, out: &Path) -> Result<(), String> {
    let mut model = wheel::WheelModel::new(mode, items);
    model.selected = selected;
    model.settle_liquid();
    // Debug renders can catch the highlight mid-flow: SATSUMA_FLOW="<from petal>,<seconds>".
    if let Some((from, t)) = std::env::var("SATSUMA_FLOW").ok().and_then(|v| {
        let (a, b) = v.split_once(',')?;
        Some((a.trim().parse::<usize>().ok()?, b.trim().parse::<f32>().ok()?))
    }) {
        let n = model.items.len();
        model.liquid.settle_on(wheel::petal_sector(from.min(n.saturating_sub(1)), n));
        model.aim_liquid();
        let mut done = 0.0;
        while done < t {
            let dt = (t - done).min(1.0 / 120.0);
            model.liquid.advance(dt);
            done += dt;
        }
    }
    wheel::render_png(&model, dark, out)
}

/// `--out x.ico` writes a multi-size icon; anything else a PNG of `size`.
pub fn render_icon(size: usize, out: &Path) -> Result<(), String> {
    if out.extension().is_some_and(|e| e.eq_ignore_ascii_case("ico")) {
        return icon::write_ico(out);
    }
    let px = size as u32;
    gfx::save_png(&icon::app_icon(px), px, px, out)
}

pub fn render_editor(kind: &str, files: Vec<PathBuf>, sets: &[(String, f64)], segment: Option<isize>, out: &Path) -> Result<(), String> {
    let dark = std::env::var_os("SATSUMA_DARK").is_some();
    editor::render_png(kind, files, sets, segment, dark, out)
}

/// A tool's options window as a PNG (`satsuma render-form`).
pub fn render_form(tool: &str, files: Vec<PathBuf>, sets: &[(u32, f64)], out: &Path) -> Result<(), String> {
    let tool = satsuma_core::Tool::from_id(tool).ok_or("Unknown tool")?;
    let form = forms::form_for(tool, &files).ok_or("This tool has no options window for these files.")?;
    let dark = std::env::var_os("SATSUMA_DARK").is_some();
    let pal = theme::Palette::for_appearance(render_theme(), dark);
    form::render_png(form, sets, &pal, out)
}

/// `SATSUMA_THEME=glass|smoke|solid` picks the look of offscreen renders (default solid).
fn render_theme() -> satsuma_core::settings::Appearance {
    use satsuma_core::settings::Appearance;
    match std::env::var("SATSUMA_THEME").unwrap_or_default().as_str() {
        "liquid" | "glass" => Appearance::Liquid,
        _ => Appearance::Solid,
    }
}

pub fn ffmpeg_hint() -> &'static str {
    "Install ffmpeg (`winget install Gyan.FFmpeg`) for audio and video."
}
