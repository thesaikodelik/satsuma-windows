//! Image conversions and tools: macOS `sips` (ImageIO; emulated on Windows by
//! [`crate::win::sips`]), with ffmpeg for WebP, PNG quantisation and collages.

use crate::env::Env;
use crate::media::Ff;
#[cfg_attr(windows, allow(unused_imports))]
use crate::process::{run, Cancel};
use crate::{Error, Result};
use satsuma_core::settings::Strength;
use satsuma_core::Format::{self, *};
use std::path::{Path, PathBuf};
#[cfg_attr(windows, allow(unused_imports))]
use std::process::Command;

/// `sips` format name for an output format.
pub fn sips_format(f: Format) -> Option<&'static str> {
    Some(match f {
        Jpg => "jpeg",
        Png => "png",
        Heic => "heic",
        Tiff => "tiff",
        Avif => "avif",
        Bmp => "bmp",
        Ico => "ico",
        Pdf => "pdf",
        Gif => "gif",
        _ => return None,
    })
}

/// Runs `sips` to write `out` in format `target`.
pub fn sips(
    env: &Env,
    input: &Path,
    out: &Path,
    target: Format,
    format_options: Option<&str>,
    max_edge: Option<u32>,
    cancel: &Cancel,
) -> Result<()> {
    let name = sips_format(target)
        .ok_or_else(|| Error::msg(format!("Cannot write {} with sips.", target.label())))?;
    #[cfg(windows)]
    {
        let _ = name;
        crate::win::sips::convert(env, input, out, target, format_options, max_edge, cancel)?;
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new(&env.sips);
    cmd.args(["-s", "format", name]);
    if let Some(o) = format_options {
        cmd.args(["-s", "formatOptions", o]);
    }
    if let Some(px) = max_edge {
        cmd.arg("-Z").arg(px.to_string());
    }
    cmd.arg(input).arg("--out").arg(out);
    run(&mut cmd, cancel, &mut |_| {})?;
    }
    if !out.exists() || std::fs::metadata(out)?.len() == 0 {
        return Err(Error::msg(format!(
            "Could not read the image {}.",
            input.file_name().unwrap_or_default().to_string_lossy()
        )));
    }
    Ok(())
}

/// Decodes any readable image (HEIC, SVG, AVIF …) to a PNG in `tmp`.
pub fn to_png(env: &Env, input: &Path, tmp: &Path, cancel: &Cancel) -> Result<PathBuf> {
    let name = format!(
        "{}.png",
        input.file_stem().unwrap_or_default().to_string_lossy()
    );
    let out = unique_in(tmp, &name);
    sips(env, input, &out, Png, None, None, cancel)?;
    Ok(out)
}

pub(crate) fn unique_in(dir: &Path, name: &str) -> PathBuf {
    let mut candidate = dir.join(name);
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{n}-{name}"));
        n += 1;
    }
    candidate
}

pub(crate) fn webp_encode(env: &Env, png: &Path, out: &Path, quality: u32, max_edge: Option<u32>, cancel: &Cancel) -> Result<()> {
    if !env.has_encoder("libwebp") {
        return Err(Error::msg("This ffmpeg build cannot encode WebP images."));
    }
    let mut ff = Ff::new(env).input(png);
    if let Some(px) = max_edge {
        ff = ff
            .arg("-vf")
            .arg(format!("scale='min({px},iw)':'min({px},ih)':force_original_aspect_ratio=decrease"));
    }
    ff.args(["-c:v", "libwebp", "-quality"])
        .arg(quality.to_string())
        .args(["-frames:v", "1"])
        .run(out, cancel, &mut |_| {})
}

/// Image → image (or PDF).
pub fn convert(env: &Env, input: &Path, out: &Path, target: Format, tmp: &Path, cancel: &Cancel) -> Result<()> {
    match target {
        Webp => {
            let png = to_png(env, input, tmp, cancel)?;
            webp_encode(env, &png, out, 90, None, cancel)
        }
        Jpg | Heic | Avif => sips(env, input, out, target, Some("90"), None, cancel),
        _ => sips(env, input, out, target, None, None, cancel),
    }
}

/// Compress an image. Fails if the result would not be smaller than the original.
pub fn compress(env: &Env, input: &Path, in_fmt: Format, out: &Path, strength: Strength, max_edge: Option<u32>, tmp: &Path, cancel: &Cancel) -> Result<()> {
    let strong = strength == Strength::Strong;
    match in_fmt {
        Jpg => sips(env, input, out, Jpg, Some(if strong { "45" } else { "70" }), max_edge, cancel)?,
        Heic => sips(env, input, out, Heic, Some(if strong { "40" } else { "60" }), max_edge, cancel)?,
        Avif => sips(env, input, out, Avif, Some(if strong { "40" } else { "60" }), max_edge, cancel)?,
        Tiff => sips(env, input, out, Tiff, Some("lzw"), max_edge, cancel)?,
        Webp => {
            let png = to_png(env, input, tmp, cancel)?;
            webp_encode(env, &png, out, if strong { 45 } else { 70 }, max_edge, cancel)?
        }
        Png => {
            if !env.has_ffmpeg() {
                return Err(Error::msg("PNG compression needs ffmpeg."));
            }
            let colors = if strong { 96 } else { 256 };
            let scale = max_edge
                .map(|px| format!("scale='min({px},iw)':'min({px},ih)':force_original_aspect_ratio=decrease,"))
                .unwrap_or_default();
            Ff::new(env)
                .input(input)
                .arg("-filter_complex")
                .arg(format!(
                    "[0:v]{scale}split[a][b];[a]palettegen=max_colors={colors}:reserve_transparent=1:stats_mode=full[p];[b][p]paletteuse=dither=sierra2_4a:alpha_threshold=128"
                ))
                .args(["-frames:v", "1", "-compression_level", "9"])
                .run(out, cancel, &mut |_| {})?
        }
        other => return Err(Error::msg(format!("Cannot compress {} images.", other.label()))),
    }
    Ok(())
}

/// Compress an image to about `target` bytes: lower quality step by step (and, for PNG,
/// fewer colours) until it fits.
#[allow(clippy::too_many_arguments)]
pub fn compress_to_size(env: &Env, input: &Path, in_fmt: Format, out: &Path, target: u64, max_edge: Option<u32>, tmp: &Path, cancel: &Cancel) -> Result<()> {
    // Lower the quality first; when that isn't enough, make the picture smaller too.
    let long = crate::probe::probe(env, input).ok().and_then(|p| p.video).map(|(w, h)| w.max(h)).unwrap_or(4000);
    let start = max_edge.unwrap_or(long).min(long);
    let scales = [1.0, 0.8, 0.64, 0.5, 0.4, 0.3, 0.22];
    let qualities = [85u32, 72, 60, 48, 36];
    let mut last: Option<PathBuf> = None;
    for scale in scales {
        let edge = ((start as f64 * scale) as u32).max(64);
        let edge = if scale == 1.0 { max_edge } else { Some(edge) };
        for q in qualities {
            cancel.check()?;
            let attempt = unique_in(tmp, &format!("try-{q}.{}", in_fmt.extension()));
            match in_fmt {
                Jpg | Heic | Avif => sips(env, input, &attempt, in_fmt, Some(&q.to_string()), edge, cancel)?,
                Webp => {
                    let png = to_png(env, input, tmp, cancel)?;
                    webp_encode(env, &png, &attempt, q, edge, cancel)?
                }
                Png => {
                    let strength = if q >= 60 { Strength::Balanced } else { Strength::Strong };
                    compress(env, input, Png, &attempt, strength, edge, tmp, cancel)?
                }
                other => return Err(Error::msg(format!("Cannot compress {} images to a size.", other.label()))),
            }
            let size = std::fs::metadata(&attempt).map(|m| m.len()).unwrap_or(u64::MAX);
            if size <= target {
                std::fs::rename(&attempt, out).or_else(|_| std::fs::copy(&attempt, out).map(|_| ()))?;
                return Ok(());
            }
            last = Some(attempt);
        }
    }
    let _ = last;
    Err(Error::msg("The image can't get that small. Try a larger size."))
}

/// One PDF page per image, in order, into `out`.
pub fn images_to_pdf(env: &Env, inputs: &[PathBuf], out: &Path, tmp: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let mut pages = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        let page = unique_in(tmp, &format!("page-{i}.pdf"));
        sips(env, input, &page, Pdf, None, None, cancel)?;
        pages.push(page);
        progress((i + 1) as f64 / (inputs.len() + 1) as f64);
    }
    crate::pdf::merge(&pages, out)?;
    progress(1.0);
    Ok(())
}

/// Grid collage of `inputs` as a PNG.
pub fn collage(env: &Env, inputs: &[PathBuf], out: &Path, tmp: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    collage_with(env, inputs, out, tmp, crate::plan::CollageLayout::Grid, 800, 16, [255, 255, 255], cancel, progress)
}

/// A collage laid out as a grid, a row or a column of `cell`-pixel squares `gap` apart on
/// `background`.
#[allow(clippy::too_many_arguments)]
pub fn collage_with(
    env: &Env,
    inputs: &[PathBuf],
    out: &Path,
    tmp: &Path,
    layout: crate::plan::CollageLayout,
    cell: u32,
    gap: u32,
    background: [u8; 3],
    cancel: &Cancel,
    progress: &mut dyn FnMut(f64),
) -> Result<()> {
    use crate::plan::CollageLayout;
    if inputs.is_empty() {
        return Err(Error::msg("Select images for the collage."));
    }
    let n = inputs.len();
    let (cols, rows) = match layout {
        CollageLayout::Grid => grid(n),
        CollageLayout::Row => (n, 1),
        CollageLayout::Column => (1, n),
    };
    #[allow(non_snake_case)]
    let (CELL, GAP) = (cell.clamp(64, 4000), gap.min(400));
    let colour = format!("0x{:02X}{:02X}{:02X}", background[0], background[1], background[2]);
    let mut pngs = Vec::new();
    for (i, input) in inputs.iter().enumerate() {
        pngs.push(to_png(env, input, tmp, cancel)?);
        progress((i + 1) as f64 / (n + 1) as f64);
    }
    let mut ff = Ff::new(env);
    for p in &pngs {
        ff = ff.input(p);
    }
    let blanks = cols * rows - n;
    for _ in 0..blanks {
        ff = ff.args(["-f", "lavfi", "-i"]).arg(format!("color=c={colour}:s={CELL}x{CELL}:d=1"));
    }
    let mut graph = String::new();
    for k in 0..n {
        graph.push_str(&format!(
            "[{k}:v]scale={CELL}:{CELL}:force_original_aspect_ratio=decrease,pad={CELL}:{CELL}:(ow-iw)/2:(oh-ih)/2:color={colour},setsar=1,format=rgb24[c{k}];"
        ));
    }
    for k in n..n + blanks {
        graph.push_str(&format!("[{k}:v]format=rgb24[c{k}];"));
    }
    let total = cols * rows;
    if total == 1 {
        graph.push_str(&format!("[c0]pad=iw+{0}:ih+{0}:{GAP}:{GAP}:color={colour}[out]", GAP * 2));
    } else {
        let layout: Vec<String> = (0..total)
            .map(|k| {
                let (c, r) = ((k % cols) as u32, (k / cols) as u32);
                format!("{}_{}", c * (CELL + GAP), r * (CELL + GAP))
            })
            .collect();
        let labels: String = (0..total).map(|k| format!("[c{k}]")).collect();
        graph.push_str(&format!(
            "{labels}xstack=inputs={total}:layout={}:fill={colour},pad=iw+{}:ih+{}:{GAP}:{GAP}:color={colour}[out]",
            layout.join("|"),
            GAP * 2,
            GAP * 2
        ));
    }
    ff.arg("-filter_complex")
        .arg(graph)
        .args(["-map", "[out]", "-frames:v", "1"])
        .run(out, cancel, progress)
}

/// Columns and rows for a near-square grid.
pub fn grid(n: usize) -> (usize, usize) {
    let cols = (n as f64).sqrt().ceil().max(1.0) as usize;
    let rows = n.div_ceil(cols);
    (cols, rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_are_near_square() {
        assert_eq!(grid(1), (1, 1));
        assert_eq!(grid(2), (2, 1));
        assert_eq!(grid(3), (2, 2));
        assert_eq!(grid(4), (2, 2));
        assert_eq!(grid(5), (3, 2));
        assert_eq!(grid(9), (3, 3));
        assert_eq!(grid(10), (4, 3));
    }

    #[test]
    fn sips_names() {
        assert_eq!(sips_format(Jpg), Some("jpeg"));
        assert_eq!(sips_format(Webp), None);
    }
}
