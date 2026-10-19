//! Local conversion backends for Satsuma.
//!
//! Everything runs locally. On macOS: `sips` (ImageIO), `textutil`, `tar`, PDFKit, CoreGraphics,
//! CoreText and Vision. On Windows: WIC and pure-Rust codecs, `Windows.Data.Pdf`,
//! `Windows.Media.Ocr`, Microsoft Edge (HTML → PDF) and the built-in `tar.exe`. Both use a
//! user-installed ffmpeg for audio and video.

pub mod archive;
pub mod capability;
pub mod doc;
pub mod docx;
pub mod env;
pub mod epub;
pub mod html;
pub mod image;
#[cfg_attr(windows, path = "win/imaging.rs")]
pub mod imaging;
pub mod media;
pub mod metadata;
pub mod ocr_layout;
#[cfg_attr(windows, path = "win/ocr.rs")]
pub mod ocr;
#[cfg_attr(windows, path = "win/pdf.rs")]
pub mod pdf;
#[cfg_attr(windows, path = "win/photo.rs")]
pub mod photo;
pub mod plan;
pub mod probe;
pub mod process;
pub mod run;
pub mod sheet;
pub mod subtitle;
#[cfg(windows)]
pub mod win;

pub use env::Env;
pub use plan::{detect_format, plan, Job, Operation, Plan};
pub use process::Cancel;
pub use run::{run_job, run_plan, BatchProgress, BatchReport, Context};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error("Cancelled")]
    Cancelled,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Error::Message(s.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
