//! Satsuma: a drag-gesture file converter for the macOS menu bar and the Windows
//! notification area.
//!
//! Without arguments it starts the app; with arguments it is a command-line tool.

mod cli;

#[cfg(target_os = "macos")]
mod app;
#[cfg(all(target_os = "macos", feature = "demo"))]
mod demo;
#[cfg(target_os = "macos")]
mod draw;
#[cfg(target_os = "macos")]
mod editor;
#[cfg(target_os = "macos")]
mod events;
#[cfg(target_os = "macos")]
mod float;
#[cfg(target_os = "macos")]
mod hud;
#[cfg(target_os = "macos")]
mod icon;
#[cfg(target_os = "macos")]
mod system;
#[cfg(target_os = "macos")]
mod wheel;
#[cfg(target_os = "macos")]
mod platform;

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as platform;

/// Starts the app, or runs a command when there are arguments.
pub fn main_entry() -> i32 {
    // Finder may pass a process serial number argument on older systems.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    if args.is_empty() {
        platform::run();
        0
    } else {
        platform::attach_console();
        cli::run(&args)
    }
}
