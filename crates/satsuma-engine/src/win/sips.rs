//! What `sips -s format <f> [-s formatOptions <q>] [-Z <px>] in --out out` does on macOS:
//! decode any image upright, optionally fit the longest edge, and encode it.

use super::raster;
use crate::env::Env;
use crate::process::Cancel;
use crate::Result;
use satsuma_core::Format;
use std::path::Path;

/// `format_options` is a quality percentage ("90") or "lzw" for TIFF, as with sips.
pub fn convert(
    _env: &Env,
    input: &Path,
    out: &Path,
    target: Format,
    format_options: Option<&str>,
    max_edge: Option<u32>,
    cancel: &Cancel,
) -> Result<()> {
    cancel.check()?;
    let img = raster::fit(raster::load(input)?, max_edge);
    cancel.check()?;
    let lzw = format_options == Some("lzw");
    let quality = format_options
        .and_then(|o| o.parse::<f32>().ok())
        .map(|q| q / 100.0)
        .unwrap_or(0.92);
    raster::save_with_profile(&img, out, target, quality, lzw, raster::icc_profile(input))
}
