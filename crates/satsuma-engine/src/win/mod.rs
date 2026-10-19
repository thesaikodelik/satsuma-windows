//! Windows backends. They stand in for the macOS system tools behind the same interfaces:
//! [`sips`] for `sips`, [`textutil`] for `textutil`, and `win/pdf.rs`, `win/imaging.rs`,
//! `win/photo.rs` and `win/ocr.rs` for PDFKit, CoreGraphics, CoreImage and Vision.

pub mod browser;
pub mod docx_read;
pub mod raster;
pub mod rtf;
pub mod sips;
pub mod textutil;
pub mod wic;

use crate::Env;
use std::sync::OnceLock;

/// The tool environment, for backends whose callers do not pass one (fallback decoding).
pub fn env() -> &'static Env {
    static ENV: OnceLock<Env> = OnceLock::new();
    ENV.get_or_init(Env::discover)
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
