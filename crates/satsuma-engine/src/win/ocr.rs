//! On-device text recognition with `Windows.Media.Ocr` (the engine behind the Snipping
//! Tool's text actions). Uses the user's display languages, falling back to English.

use crate::win::{ensure_com, raster, winerr};
use crate::{Error, Result};
use std::path::Path;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::FaceAnalysis::FaceDetector;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};

pub use crate::ocr_layout::{paragraphs, table_rows, Line, Word};

fn engine() -> Result<OcrEngine> {
    if let Ok(e) = OcrEngine::TryCreateFromUserProfileLanguages() {
        return Ok(e);
    }
    let english = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| winerr("Text recognition is unavailable", e))?;
    OcrEngine::TryCreateFromLanguage(&english).map_err(|_| {
        Error::msg("Text recognition needs an OCR language. Add one in Settings › Time & language › Language & region.")
    })
}

/// The image as an upright software bitmap no larger than `max`, and its size.
fn bitmap(path: &Path, max: u32, format: BitmapPixelFormat, what: &'static str) -> Result<(SoftwareBitmap, f64, f64)> {
    let img = raster::fit(raster::load(path)?, Some(max));
    let (w, h) = (img.width() as f64, img.height() as f64);
    let dir = tempfile::tempdir()?;
    let png = dir.path().join("scan.png");
    raster::save(&img, &png, satsuma_core::Format::Png, 1.0, false)?;
    let fail = |e| winerr(what, e);
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(png.as_os_str())).and_then(|op| op.join()).map_err(fail)?;
    let stream = file.OpenAsync(FileAccessMode::Read).and_then(|op| op.join()).map_err(fail)?;
    let decoder = BitmapDecoder::CreateAsync(&stream).and_then(|op| op.join()).map_err(fail)?;
    let bitmap = decoder
        .GetSoftwareBitmapConvertedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied)
        .and_then(|op| op.join())
        .map_err(fail)?;
    // The decoder only hands out colour; other formats (Gray8 for faces) are converted after.
    let bitmap = if format == BitmapPixelFormat::Bgra8 { bitmap } else { SoftwareBitmap::Convert(&bitmap, format).map_err(fail)? };
    Ok((bitmap, w, h))
}

fn ocr_result(path: &Path) -> Result<(windows::Media::Ocr::OcrResult, f64, f64)> {
    ensure_com();
    let engine = engine()?;
    // Normalise to an upright PNG no larger than the engine accepts.
    let max = OcrEngine::MaxImageDimension().unwrap_or(4096).max(1024);
    let (bitmap, w, h) = bitmap(path, max, BitmapPixelFormat::Bgra8, "Text recognition failed")?;
    let result = engine.RecognizeAsync(&bitmap).and_then(|op| op.join()).map_err(|e| winerr("Text recognition failed", e))?;
    Ok((result, w, h))
}

/// A box around each recognised word: (x, y, width, height) as fractions, top-left origin.
pub fn word_boxes(path: &Path) -> Result<Vec<[f64; 4]>> {
    let (result, w, h) = ocr_result(path)?;
    let fail = |e| winerr("Text recognition failed", e);
    let mut boxes = Vec::new();
    for line in result.Lines().map_err(fail)? {
        for word in line.Words().map_err(fail)? {
            let r = word.BoundingRect().map_err(fail)?;
            boxes.push([r.X as f64 / w, r.Y as f64 / h, r.Width as f64 / w, r.Height as f64 / h]);
        }
    }
    Ok(boxes)
}

/// Every recognised word with its box, in pixels of the image as given.
pub fn words(path: &Path) -> Result<Vec<Word>> {
    let (result, w, _) = ocr_result(path)?;
    // The engine may have seen a smaller copy; scale boxes back to the original.
    let k = raster::load(path).map(|img| img.width() as f64 / w).unwrap_or(1.0);
    let fail = |e| winerr("Text recognition failed", e);
    let mut out = Vec::new();
    for line in result.Lines().map_err(fail)? {
        for word in line.Words().map_err(fail)? {
            let r = word.BoundingRect().map_err(fail)?;
            let text = word.Text().map_err(fail)?.to_string();
            out.push(Word { text, x: r.X as f64 * k, y: r.Y as f64 * k, w: r.Width as f64 * k, h: r.Height as f64 * k });
        }
    }
    Ok(out)
}

/// A box around each face found: (x, y, width, height) as fractions, top-left origin.
pub fn face_boxes(path: &Path) -> Result<Vec<[f64; 4]>> {
    ensure_com();
    let fail = |e| winerr("Face detection failed", e);
    if !FaceDetector::IsSupported().unwrap_or(false) {
        return Err(Error::msg("Face detection isn't available on this PC."));
    }
    let (bitmap, w, h) = bitmap(path, 2560, BitmapPixelFormat::Gray8, "Face detection failed")?;
    let detector = FaceDetector::CreateAsync().and_then(|op| op.join()).map_err(fail)?;
    let faces = detector.DetectFacesAsync(&bitmap).and_then(|op| op.join()).map_err(fail)?;
    let mut boxes = Vec::new();
    for face in faces {
        let b = face.FaceBox().map_err(fail)?;
        boxes.push([b.X as f64 / w, b.Y as f64 / h, b.Width as f64 / w, b.Height as f64 / h]);
    }
    Ok(boxes)
}

pub fn recognize(path: &Path) -> Result<Vec<Line>> {
    let (result, w, h) = ocr_result(path)?;
    let fail = |e| winerr("Text recognition failed", e);

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
