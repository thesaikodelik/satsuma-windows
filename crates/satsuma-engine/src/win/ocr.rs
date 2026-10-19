//! On-device text recognition with `Windows.Media.Ocr` (the engine behind the Snipping
//! Tool's text actions). Uses the user's display languages, falling back to English.

use crate::win::{ensure_com, raster, winerr};
use crate::{Error, Result};
use std::path::Path;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};

pub use crate::ocr_layout::{paragraphs, Line};

fn engine() -> Result<OcrEngine> {
    if let Ok(e) = OcrEngine::TryCreateFromUserProfileLanguages() {
        return Ok(e);
    }
    let english = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| winerr("Text recognition is unavailable", e))?;
    OcrEngine::TryCreateFromLanguage(&english).map_err(|_| {
        Error::msg("Text recognition needs an OCR language. Add one in Settings › Time & language › Language & region.")
    })
}

pub fn recognize(path: &Path) -> Result<Vec<Line>> {
    ensure_com();
    let engine = engine()?;
    // Normalise to an upright PNG no larger than the engine accepts.
    let max = OcrEngine::MaxImageDimension().unwrap_or(4096).max(1024);
    let img = raster::fit(raster::load(path)?, Some(max));
    let (w, h) = (img.width() as f64, img.height() as f64);
    let dir = tempfile::tempdir()?;
    let png = dir.path().join("ocr.png");
    raster::save(&img, &png, satsuma_core::Format::Png, 1.0, false)?;

    let fail = |e| winerr("Text recognition failed", e);
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(png.as_os_str())).and_then(|op| op.join()).map_err(fail)?;
    let stream = file.OpenAsync(FileAccessMode::Read).and_then(|op| op.join()).map_err(fail)?;
    let decoder = BitmapDecoder::CreateAsync(&stream).and_then(|op| op.join()).map_err(fail)?;
    let bitmap = decoder
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied)
        .and_then(|op| op.join())
        .map_err(fail)?;
    let result = engine.RecognizeAsync(&bitmap).and_then(|op| op.join()).map_err(fail)?;

    let mut lines = Vec::new();
    for line in result.Lines().map_err(fail)? {
        let text = line.Text().map_err(fail)?.to_string();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for word in line.Words().map_err(fail)? {
            let r = word.BoundingRect().map_err(fail)?;
            x0 = x0.min(r.X as f64);
            y0 = y0.min(r.Y as f64);
            x1 = x1.max((r.X + r.Width) as f64);
            y1 = y1.max((r.Y + r.Height) as f64);
        }
        if x0 > x1 {
            continue;
        }
        lines.push(Line { text, x: x0 / w, top: y0 / h, height: (y1 - y0) / h });
    }
    Ok(lines)
}
