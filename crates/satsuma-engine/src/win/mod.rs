//! Windows backends. They stand in for the macOS system tools behind the same interfaces:
//! [`sips`] for `sips`, [`textutil`] for `textutil`, and `win/pdf.rs`, `win/imaging.rs`,
//! `win/photo.rs` and `win/ocr.rs` for PDFKit, CoreGraphics, CoreImage and Vision.

pub mod browser;
pub mod docx_read;
pub mod office;
pub mod raster;
pub mod rtf;
pub mod sips;
pub mod textutil;
pub mod wic;

use crate::Env;
use std::sync::{OnceLock, RwLock};

/// The tool environment, for backends whose callers do not pass one (fallback decoding).
pub fn env() -> &'static Env {
    let lock = ENV.get_or_init(|| RwLock::new(Box::leak(Box::new(Env::discover()))));
    *lock.read().unwrap_or_else(|e| e.into_inner())
}

static ENV: OnceLock<RwLock<&'static Env>> = OnceLock::new();

/// Looks for the tools again (after Satsuma downloaded ffmpeg). The old environment is
/// leaked on purpose: callers may still hold it, and it happens once or twice per run.
pub fn refresh_env() -> &'static Env {
    let fresh: &'static Env = Box::leak(Box::new(Env::discover()));
    let lock = ENV.get_or_init(|| RwLock::new(fresh));
    *lock.write().unwrap_or_else(|e| e.into_inner()) = fresh;
    fresh
}

/// Joins the calling thread to a COM apartment for WIC and WinRT. Threads that already
/// have one (the app's STA UI thread) keep it.
pub fn ensure_com() {
    thread_local! {
        static INIT: () = unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        };
    }
    INIT.with(|_| {});
}

/// A WinRT/COM error as a user-facing message.
pub(crate) fn winerr(context: &str, e: windows::core::Error) -> crate::Error {
    let detail = e.message();
    let detail = detail.trim().trim_end_matches('.');
    if detail.is_empty() {
        crate::Error::msg(format!("{context} (0x{:08X}).", e.code().0 as u32))
    } else {
        crate::Error::msg(format!("{context}: {detail}."))
    }
}
