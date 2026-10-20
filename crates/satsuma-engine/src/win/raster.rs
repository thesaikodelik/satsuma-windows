//! Reading and writing raster images on Windows: pure-Rust codecs first, then WIC (HEIC,
//! AVIF and camera formats through installed extensions), then ffmpeg as a last resort.

use super::wic;
use crate::process::Cancel;
use crate::{Error, Result};
use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader, RgbaImage};
use satsuma_core::Format;
use std::io::BufWriter;
use std::path::Path;

fn name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// Applies an EXIF orientation value (1–8).
pub fn orient(img: RgbaImage, orientation: u16) -> RgbaImage {
    let mut d = DynamicImage::ImageRgba8(img);
    if let Some(o) = image::metadata::Orientation::from_exif(orientation as u8) {
        d.apply_orientation(o);
    }
    d.into_rgba8()
}

fn decode_with_image_crate(path: &Path) -> Option<RgbaImage> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    reader.format()?;
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Some(img.into_rgba8())
}

fn decode_svg(path: &Path) -> Result<RgbaImage> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(path)?;
    let mut opts = usvg::Options { resources_dir: path.parent().map(Path::to_path_buf), ..Default::default() };
    opts.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(&data, &opts).map_err(|e| Error::msg(format!("Could not read the SVG {}: {e}.", name(path))))?;
    let size = tree.size();
    // Small icons are rendered larger, like sips does for vector art.
    let scale = if size.width().max(size.height()) < 512.0 { 512.0 / size.width().max(size.height()) } else { 1.0 };
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error::msg("The SVG is too large."))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(super::super::imaging::from_pixmap(&pixmap))
}

fn decode_with_ffmpeg(path: &Path) -> Result<RgbaImage> {
    let env = super::env();
    if !env.has_ffmpeg() {
        return Err(Error::msg(format!("Could not read the image {}.", name(path))));
    }
    let dir = tempfile::tempdir()?;
    let png = dir.path().join("frame.png");
    crate::media::Ff::new(env)
        .input(path)
        .args(["-frames:v", "1", "-update", "1"])
        .run(&png, &Cancel::new(), &mut |_| {})
        .map_err(|_| Error::msg(format!("Could not read the image {}.", name(path))))?;
    decode_with_image_crate(&png).ok_or_else(|| Error::msg(format!("Could not read the image {}.", name(path))))
}

/// Loads the first image of any readable file, upright, as straight RGBA8.
pub fn load(path: &Path) -> Result<RgbaImage> {
    if !path.is_file() {
        return Err(Error::msg(format!("Could not read the image {}.", name(path))));
    }
    if Format::from_path(path) == Some(Format::Svg) {
        return decode_svg(path);
    }
    if let Some(img) = decode_with_image_crate(path) {
        return Ok(img);
    }
    match wic::decode(path) {
        Ok(img) => Ok(img),
        Err(wic_err) => decode_with_ffmpeg(path).map_err(|_| wic_err),
    }
}

/// The embedded colour profile, when the image crate can read the file.
pub fn icc_profile(path: &Path) -> Option<Vec<u8>> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    reader.format()?;
    let mut decoder = reader.into_decoder().ok()?;
    decoder.icc_profile().ok().flatten().filter(|p| !p.is_empty())
}

/// Fits the longest edge within `max_edge` (never enlarges).
pub fn fit(img: RgbaImage, max_edge: Option<u32>) -> RgbaImage {
    let Some(px) = max_edge else { return img };
    let (w, h) = img.dimensions();
    let long = w.max(h);
    if long <= px {
        return img;
    }
    let s = px as f64 / long as f64;
    let (nw, nh) = (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1));
    image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3)
}

fn has_alpha(img: &RgbaImage) -> bool {
    img.pixels().any(|p| p.0[3] < 255)
}

/// Flattens transparency onto white (for formats without alpha).
pub fn flatten(img: &RgbaImage) -> image::RgbImage {
    image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let [r, g, b, a] = img.get_pixel(x, y).0;
        let a = a as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        image::Rgb([mix(r), mix(g), mix(b)])
    })
}

fn create(path: &Path) -> Result<BufWriter<std::fs::File>> {
    Ok(BufWriter::new(std::fs::File::create(path)?))
}

fn encode_err(e: impl std::fmt::Display) -> Error {
    Error::msg(format!("Could not save the image: {e}"))
}

/// Writes `img` as `format`. `quality` is 0–1 for lossy formats; `lzw` compresses TIFFs.
pub fn save(img: &RgbaImage, path: &Path, format: Format, quality: f32, lzw: bool) -> Result<()> {
    save_with_profile(img, path, format, quality, lzw, None)
}

/// [`save`], embedding `icc` (the source's colour profile) in JPEG and PNG output so
/// wide-gamut photos keep their colours, as sips does.
pub fn save_with_profile(img: &RgbaImage, path: &Path, format: Format, quality: f32, lzw: bool, icc: Option<Vec<u8>>) -> Result<()> {
    use image::codecs::*;
    match format {
        Format::Jpg => {
            let q = (quality * 100.0).round().clamp(1.0, 100.0) as u8;
            let mut enc = jpeg::JpegEncoder::new_with_quality(create(path)?, q);
            if let Some(icc) = icc {
                let _ = enc.set_icc_profile(icc);
            }
            enc.encode_image(&flatten(img)).map_err(encode_err)?;
        }
        Format::Png => {
            let mut enc = png::PngEncoder::new_with_quality(create(path)?, png::CompressionType::Best, png::FilterType::Adaptive);
            if let Some(icc) = icc {
                let _ = enc.set_icc_profile(icc);
            }
            if has_alpha(img) {
                img.write_with_encoder(enc).map_err(encode_err)?;
            } else {
                flatten(img).write_with_encoder(enc).map_err(encode_err)?;
            }
        }
        Format::Bmp => {
            let mut w = create(path)?;
            bmp::BmpEncoder::new(&mut w).encode(flatten(img).as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8).map_err(encode_err)?;
        }
        Format::Gif => {
            let mut enc = gif::GifEncoder::new_with_speed(create(path)?, 10);
            enc.encode_frame(image::Frame::new(img.clone())).map_err(encode_err)?;
        }
        Format::Tiff => write_tiff(img, path, lzw)?,
        Format::Heic => {
            if !wic::can_encode_heif() {
                return Err(Error::msg("Saving HEIC needs the HEIF and HEVC Video Extensions from the Microsoft Store."));
            }
            wic::encode(img, path, &windows::Win32::Graphics::Imaging::GUID_ContainerFormatHeif, quality)?;
        }
        Format::Avif => encode_avif(img, path, quality)?,
        Format::Pdf => crate::pdf::image_pdf(img, path)?,
        Format::Webp => {
            let dir = tempfile::tempdir()?;
            let png = dir.path().join("image.png");
            save(img, &png, Format::Png, 1.0, false)?;
            crate::image::webp_encode(super::env(), &png, path, (quality * 100.0).round() as u32, None, &Cancel::new())?;
        }
        other => return Err(Error::msg(format!("Cannot save {} images here.", other.label()))),
    }
    Ok(())
}

fn write_tiff(img: &RgbaImage, path: &Path, lzw: bool) -> Result<()> {
    use tiff::encoder::{colortype, Compression, TiffEncoder};
    let compression = if lzw { Compression::Lzw } else { Compression::Uncompressed };
    let mut enc = TiffEncoder::new(create(path)?).map_err(encode_err)?.with_compression(compression);
    let (w, h) = img.dimensions();
    let result = if has_alpha(img) {
        enc.write_image::<colortype::RGBA8>(w, h, img.as_raw())
    } else {
        enc.write_image::<colortype::RGB8>(w, h, flatten(img).as_raw())
    };
    result.map_err(encode_err)
}

/// AVIF encoders that ffmpeg may have, best first.
pub const AVIF_ENCODERS: [&str; 3] = ["libaom-av1", "libsvtav1", "librav1e"];

/// AVIF still images are written with ffmpeg's AV1 encoders.
fn encode_avif(img: &RgbaImage, path: &Path, quality: f32) -> Result<()> {
    let env = super::env();
    let encoder = env
        .pick_encoder(&AVIF_ENCODERS)
        .ok_or_else(|| Error::msg("Saving AVIF needs ffmpeg with an AV1 encoder."))?;
    let dir = tempfile::tempdir()?;
    let png = dir.path().join("image.png");
    save(img, &png, Format::Png, 1.0, false)?;
    // Map quality 0–1 onto CRF 63–0 (libaom/SVT) or quantizer 255–0 (rav1e).
    let mut ff = crate::media::Ff::new(env).input(&png).args(["-frames:v", "1", "-c:v", encoder]);
    let pix = if has_alpha(img) { "yuva420p" } else { "yuv420p" };
    ff = match encoder {
        "librav1e" => ff.arg("-qp").arg(((1.0 - quality) * 255.0).round().to_string()),
        "libsvtav1" => ff.arg("-crf").arg(((1.0 - quality) * 63.0).round().to_string()).args(["-pix_fmt", "yuv420p"]),
        _ => ff.arg("-crf").arg(((1.0 - quality) * 63.0).round().to_string()).args(["-still-picture", "1", "-pix_fmt", pix]),
    };
    ff.args(["-f", "avif"]).run(path, &Cancel::new(), &mut |_| {})
}

/// Whether this PC can write `format` as an image.
pub fn can_write(format: Format) -> bool {
    match format {
        Format::Jpg | Format::Png | Format::Bmp | Format::Gif | Format::Tiff | Format::Pdf => true,
        Format::Heic => wic::can_encode_heif(),
        Format::Avif => super::env().pick_encoder(&AVIF_ENCODERS).is_some(),
        Format::Webp => super::env().has_encoder("libwebp"),
        _ => false,
    }
}
