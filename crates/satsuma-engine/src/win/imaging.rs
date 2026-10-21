//! In-process image editing on Windows: upright loading, cropping, Add Background
//! compositing (tiny-skia) and writing. Mirrors the CoreGraphics version in `imaging.rs`.

use crate::win::raster;
use crate::{Error, Result};
use image::RgbaImage;
use resvg::tiny_skia::{
    self, Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, PathBuilder, Pattern, Pixmap, Point,
    PremultipliedColorU8, SpreadMode, Transform,
};
use satsuma_core::edit::{frame_layout, Background, FrameSpec, Rect, GRADIENTS};
use satsuma_core::Format;
use std::path::Path;
use std::sync::Arc;

/// A decoded image: straight (not premultiplied) sRGB RGBA8, shared cheaply.
pub type Image = Arc<RgbaImage>;

pub fn size(img: &RgbaImage) -> (f64, f64) {
    (img.width() as f64, img.height() as f64)
}

/// Loads the first image of a file, upright (EXIF orientation applied), at full size.
pub fn load(path: &Path) -> Result<Image> {
    Ok(Arc::new(raster::load(path)?))
}

/// Crops `rect` (pixels, top-left origin) and optionally resizes to `output`.
pub fn crop(img: &RgbaImage, rect: Rect, output: Option<(u32, u32)>) -> Result<Image> {
    let (w, h) = img.dimensions();
    let x = (rect.x.round().max(0.0) as u32).min(w.saturating_sub(1));
    let y = (rect.y.round().max(0.0) as u32).min(h.saturating_sub(1));
    let cw = (rect.w.round().max(1.0) as u32).min(w - x);
    let ch = (rect.h.round().max(1.0) as u32).min(h - y);
    if cw == 0 || ch == 0 {
        return Err(Error::msg("Could not create the cropped image."));
    }
    let cropped = image::imageops::crop_imm(img, x, y, cw, ch).to_image();
    Ok(Arc::new(match output {
        Some((ow, oh)) if (ow, oh) != (cw, ch) => {
            image::imageops::resize(&cropped, ow.max(1), oh.max(1), image::imageops::FilterType::Lanczos3)
        }
        _ => cropped,
    }))
}

/// Straight RGBA → a premultiplied tiny-skia pixmap.
pub fn to_pixmap(img: &RgbaImage) -> Result<Pixmap> {
    let mut pm = Pixmap::new(img.width().max(1), img.height().max(1)).ok_or_else(|| Error::msg("The image is too large."))?;
    for (dst, src) in pm.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        let c = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
        *dst = c;
    }
    Ok(pm)
}

/// A premultiplied tiny-skia pixmap → straight RGBA.
pub fn from_pixmap(pm: &Pixmap) -> RgbaImage {
    let mut out = RgbaImage::new(pm.width(), pm.height());
    for (dst, src) in out.pixels_mut().zip(pm.pixels()) {
        let c = src.demultiply();
        dst.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    out
}

/// A rounded rectangle path (`r` clamped to half the short side).
pub fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let mut pb = PathBuilder::new();
    if r <= 0.0 {
        pb.push_rect(tiny_skia::Rect::from_xywh(x, y, w, h)?);
        return pb.finish();
    }
    // Cubic approximation of quarter circles.
    let k = r * 0.552_284_8;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Separable box blur, three passes ≈ a Gaussian with the given sigma, on one channel.
fn blur_alpha(alpha: &mut [f32], w: usize, h: usize, sigma: f32) {
    if sigma < 0.5 {
        return;
    }
    // Box width per pass for three passes (Kovesi).
    let n = 3.0f32;
    let ideal = ((12.0 * sigma * sigma / n) + 1.0).sqrt();
    let radius = (((ideal - 1.0) / 2.0).floor().max(0.0)) as usize;
    let mut tmp = vec![0f32; alpha.len()];
    for _ in 0..3 {
        box_pass(alpha, &mut tmp, w, h, radius, true);
        box_pass(&tmp, alpha, w, h, radius, false);
    }
}

fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    let idx = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
    let norm = 1.0 / (2 * r + 1) as f32;
    for line in 0..lines {
        let mut acc = 0f32;
        for i in 0..=r.min(len.saturating_sub(1)) {
            acc += src[idx(line, i)];
        }
        for i in 0..len {
            dst[idx(line, i)] = acc * norm;
            let add = i + r + 1;
            if add < len {
                acc += src[idx(line, add)];
            }
            if i >= r {
                acc -= src[idx(line, i - r)];
            }
        }
    }
}

fn color(c: (u8, u8, u8)) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, 255)
}

/// Composites the photo onto a background. `scale` shrinks everything for live previews.
pub fn frame(img: &RgbaImage, spec: &FrameSpec, background: Option<&RgbaImage>, scale: f64) -> Result<Image> {
    let (iw, ih) = size(img);
    let (iw, ih) = ((iw * scale).round().max(1.0), (ih * scale).round().max(1.0));
    let ((cw, ch), place) = frame_layout((iw, ih), spec.padding * scale, spec.ratio);
    let (cwu, chu) = (cw.round().max(1.0) as u32, ch.round().max(1.0) as u32);
    let mut canvas = Pixmap::new(cwu, chu).ok_or_else(|| Error::msg("Could not create the image canvas."))?;
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, cw as f32, ch as f32).unwrap();

    match (&spec.background, background) {
        (Background::Image(_), Some(bg)) => {
            let bgp = to_pixmap(bg)?;
            let (bw, bh) = size(bg);
            let s = (cw / bw).max(ch / bh);
            let (dw, dh) = (bw * s, bh * s);
            let t = Transform::from_row(s as f32, 0.0, 0.0, s as f32, ((cw - dw) / 2.0) as f32, ((ch - dh) / 2.0) as f32);
            let paint = Paint {
                shader: Pattern::new(bgp.as_ref(), SpreadMode::Pad, FilterQuality::Bicubic, 1.0, t),
                anti_alias: true,
                ..Default::default()
            };
            canvas.fill_rect(full, &paint, Transform::identity(), None);
        }
        (Background::Solid { r, g, b }, _) => canvas.fill(color((*r, *g, *b))),
        (bg, _) => {
            let preset = match bg {
                Background::Gradient(i) => GRADIENTS[(*i).min(GRADIENTS.len() - 1)],
                _ => GRADIENTS[0],
            };
            let stops = vec![
                GradientStop::new(0.0, color(preset.stops[0])),
                GradientStop::new(0.5, color(preset.stops[1])),
                GradientStop::new(1.0, color(preset.stops[2])),
            ];
            // Top-left to bottom-right.
            let shader = LinearGradient::new(Point::from_xy(0.0, 0.0), Point::from_xy(cw as f32, ch as f32), stops, SpreadMode::Pad, Transform::identity())
                .ok_or_else(|| Error::msg("Could not prepare the image background."))?;
            let paint = Paint { shader, ..Default::default() };
            canvas.fill_rect(full, &paint, Transform::identity(), None);
        }
    }

    let (px, py, pw, ph) = (place.x as f32, place.y as f32, place.w as f32, place.h as f32);
    let radius = ((spec.corners * scale) as f32).clamp(0.0, (pw.min(ph) / 2.0 - 0.5).max(0.0));
    let path = rounded_rect(px, py, pw, ph, radius).ok_or_else(|| Error::msg("Could not render the image."))?;

    let shadow = (spec.shadow * scale) as f32;
    if shadow > 0.0 {
        // A blurred, offset copy of the photo's shape at 35 % black (CoreGraphics-style blur).
        let mut shape = Pixmap::new(cwu, chu).unwrap();
        let mut paint = Paint::default();
        paint.set_color(Color::BLACK);
        paint.anti_alias = true;
        shape.fill_path(&path, &paint, FillRule::Winding, Transform::from_translate(0.0, shadow * 0.35), None);
        let mut alpha: Vec<f32> = shape.pixels().iter().map(|p| p.alpha() as f32).collect();
        blur_alpha(&mut alpha, cwu as usize, chu as usize, shadow / 2.0);
        for (p, a) in shape.pixels_mut().iter_mut().zip(alpha) {
            let a = (a * 0.35).round().clamp(0.0, 255.0) as u8;
            *p = PremultipliedColorU8::from_rgba(0, 0, 0, a).unwrap();
        }
        canvas.draw_pixmap(0, 0, shape.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
    }

    let photo = to_pixmap(img)?;
    let sx = pw / img.width() as f32;
    let sy = ph / img.height() as f32;
    let paint = Paint {
        shader: Pattern::new(photo.as_ref(), SpreadMode::Pad, FilterQuality::Bicubic, 1.0, Transform::from_row(sx, 0.0, 0.0, sy, px, py)),
        anti_alias: true,
        ..Default::default()
    };
    canvas.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    Ok(Arc::new(from_pixmap(&canvas)))
}

/// Type identifier of a format this backend can write (kept for parity with macOS).
pub fn uti(format: Format) -> Option<&'static str> {
    Some(match format {
        Format::Jpg => "public.jpeg",
        Format::Png => "public.png",
        Format::Heic => "public.heic",
        Format::Tiff => "public.tiff",
        Format::Bmp => "com.microsoft.bmp",
        Format::Avif => "public.avif",
        Format::Gif => "com.compuserve.gif",
        _ => return None,
    })
}

fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: std::sync::OnceLock<Arc<resvg::usvg::fontdb::Database>> = std::sync::OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            db.set_sans_serif_family("Segoe UI");
            Arc::new(db)
        })
        .clone()
}

/// Renders `svg` (whose view box is in the image's pixels) at `w`×`h`, as straight RGBA.
pub fn render_svg(svg: &str, w: u32, h: u32) -> Result<RgbaImage> {
    let opts = resvg::usvg::Options { fontdb: fonts(), ..Default::default() };
    let tree = resvg::usvg::Tree::from_str(svg, &opts).map_err(|e| Error::msg(format!("Could not draw the marks: {e}.")))?;
    let mut pm = Pixmap::new(w.max(1), h.max(1)).ok_or_else(|| Error::msg("The image is too large."))?;
    let size = tree.size();
    let t = Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, t, &mut pm.as_mut());
    Ok(from_pixmap(&pm))
}

/// `img` with the marks of `svg` (in the image's pixels) drawn over it.
pub fn overlay_svg(img: &RgbaImage, svg: &str) -> Result<Image> {
    let marks = render_svg(svg, img.width(), img.height())?;
    let mut out = img.clone();
    image::imageops::overlay(&mut out, &marks, 0, 0);
    Ok(Arc::new(out))
}

/// Writes `img` in `format` (not WebP; see `run.rs`).
pub fn write(img: &RgbaImage, path: &Path, format: Format) -> Result<()> {
    uti(format).ok_or_else(|| Error::msg(format!("Cannot save {} images here.", format.label())))?;
    raster::save(img, path, format, 0.92, false)
}

/// PNG bytes of an image (for embedding).
pub fn png_bytes(img: &RgbaImage, tmp: &Path) -> Result<Vec<u8>> {
    let p = crate::image::unique_in(tmp, "embed.png");
    write(img, &p, Format::Png)?;
    Ok(std::fs::read(p)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use satsuma_core::edit::CropSpec;

    fn sample(dir: &Path, w: u32, h: u32) -> std::path::PathBuf {
        let img = RgbaImage::from_pixel(w, h, image::Rgba([51, 128, 230, 255]));
        let p = dir.join("sample.png");
        write(&img, &p, Format::Png).unwrap();
        p
    }

    #[test]
    fn crop_and_resize() {
        let dir = tempfile::tempdir().unwrap();
        let img = load(&sample(dir.path(), 400, 300)).unwrap();
        assert_eq!(size(&img), (400.0, 300.0));
        let spec = CropSpec::centered(Some((1, 1)), (400.0, 300.0));
        let c = crop(&img, spec.pixels((400.0, 300.0)), None).unwrap();
        assert_eq!(size(&c), (300.0, 300.0));
        let r = crop(&img, Rect::new(0.0, 0.0, 200.0, 100.0), Some((50, 25))).unwrap();
        assert_eq!(size(&r), (50.0, 25.0));
    }

    #[test]
    fn frame_sizes_follow_the_layout() {
        let dir = tempfile::tempdir().unwrap();
        let img = load(&sample(dir.path(), 400, 200)).unwrap();
        let mut spec = FrameSpec::defaults_for((400.0, 200.0));
        spec.padding = 50.0;
        let out = frame(&img, &spec, None, 1.0).unwrap();
        assert_eq!(size(&out), (500.0, 300.0));
        // The photo sits in the middle, the gradient shows at the edge.
        assert_eq!(out.get_pixel(250, 150).0, [51, 128, 230, 255]);
        assert_ne!(out.get_pixel(2, 2).0, [51, 128, 230, 255]);
        spec.ratio = Some((1, 1));
        spec.background = Background::Solid { r: 255, g: 255, b: 255 };
        let out = frame(&img, &spec, None, 1.0).unwrap();
        assert_eq!(size(&out), (500.0, 500.0));
        assert_eq!(out.get_pixel(1, 1).0, [255, 255, 255, 255]);
        let preview = frame(&img, &spec, None, 0.5).unwrap();
        assert_eq!(size(&preview), (250.0, 250.0));
        let p = dir.path().join("framed.png");
        write(&out, &p, Format::Png).unwrap();
        assert_eq!(size(&load(&p).unwrap()), (500.0, 500.0));
    }
}
