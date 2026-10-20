//! Drawing for the layered windows (wheel, HUD) and offscreen renders: Direct2D into a
//! premultiplied 32-bit DIB, text with DirectWrite, icons from Segoe Fluent Icons.

use satsuma_core::geometry::PathCmd;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::core::{w, HSTRING, PCWSTR};
use windows_numerics::{Matrix3x2, Vector2 as D2D_POINT_2F};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

struct Factories {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    formats: RefCell<HashMap<(String, u32, i32, bool), IDWriteTextFormat>>,
    icon_family: &'static str,
}

thread_local! {
    // Leaked on purpose: releasing COM objects from a TLS destructor during process exit
    // runs after Direct2D may have been unloaded.
    static FACTORIES: &'static Factories = Box::leak(Box::new(Factories::new()));
}

impl Factories {
    fn new() -> Factories {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("Direct2D");
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("DirectWrite");
            let icon_family = if font_installed(&dwrite, "Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" };
            Factories { d2d, dwrite, formats: RefCell::new(HashMap::new()), icon_family }
        }
    }
}

fn font_installed(dwrite: &IDWriteFactory, family: &str) -> bool {
    unsafe {
        let mut fonts = None;
        if dwrite.GetSystemFontCollection(&mut fonts, false).is_err() {
            return false;
        }
        let Some(fonts) = fonts else { return false };
        let (mut index, mut exists) = (0u32, windows::core::BOOL(0));
        fonts.FindFamilyName(&HSTRING::from(family), &mut index, &mut exists).is_ok() && exists.as_bool()
    }
}

/// A colour with straight alpha, 0–1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const WHITE: Rgba = Rgba(1.0, 1.0, 1.0, 1.0);
    pub fn alpha(self, a: f32) -> Rgba {
        Rgba(self.0, self.1, self.2, self.3 * a)
    }
    pub fn hex(rgb: u32, a: f32) -> Rgba {
        Rgba(((rgb >> 16) & 0xff) as f32 / 255.0, ((rgb >> 8) & 0xff) as f32 / 255.0, (rgb & 0xff) as f32 / 255.0, a)
    }
    fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.0, g: self.1, b: self.2, a: self.3 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Semibold,
    Bold,
}

impl Weight {
    fn dwrite(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Weight::Regular => DWRITE_FONT_WEIGHT_NORMAL,
            Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
            Weight::Bold => DWRITE_FONT_WEIGHT_BOLD,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Leading,
    Center,
}

/// An offscreen premultiplied BGRA surface with a Direct2D render target bound to it.
pub struct Surface {
    pub width: i32,
    pub height: i32,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    rt: ID2D1DCRenderTarget,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

impl Surface {
    pub fn new(width: i32, height: i32) -> windows::core::Result<Surface> {
        let (width, height) = (width.max(1), height.max(1));
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
            let old = SelectObject(dc, bitmap.into());
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let rt = FACTORIES.with(|f| f.d2d.CreateDCRenderTarget(&props))?;
            rt.BindDC(dc, &RECT { left: 0, top: 0, right: width, bottom: height })?;
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            Ok(Surface { width, height, dc, bitmap, old, bits: bits as *mut u8, rt })
        }
    }

    /// Clears to transparent and draws with `f`, scaled by `scale` (DIPs → pixels).
    pub fn draw(&self, scale: f32, f: impl FnOnce(&Painter)) {
        unsafe {
            self.rt.BeginDraw();
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            self.rt.SetTransform(&Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: 0.0, M32: 0.0 });
            let painter = Painter { rt: self.rt.clone().into() };
            f(&painter);
            let _ = self.rt.EndDraw(None, None);
            let _ = GdiFlush();
        }
    }

    /// Shows the surface as the content of a layered window at `(x, y)` (screen pixels).
    pub fn present(&self, hwnd: HWND, x: i32, y: i32, opacity: u8) {
        unsafe {
            let pos = POINT { x, y };
            let size = SIZE { cx: self.width, cy: self.height };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: opacity, AlphaFormat: AC_SRC_ALPHA as u8 };
            let _ = UpdateLayeredWindow(hwnd, None, Some(&pos), Some(&size), Some(self.dc), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
    }

    /// Sets every pixel's alpha to at least `min` (keeps an invisible window hit-testable).
    pub fn fill_transparent(&self, min_alpha: u8) {
        let n = (self.width * self.height) as usize;
        let px = unsafe { std::slice::from_raw_parts_mut(self.bits, n * 4) };
        for p in px.chunks_exact_mut(4) {
            if p[3] < min_alpha {
                p[3] = min_alpha;
            }
        }
    }

    /// Straight-alpha RGBA pixels (for PNG export and icons).
    pub fn rgba(&self) -> Vec<u8> {
        let n = (self.width * self.height) as usize;
        let px = unsafe { std::slice::from_raw_parts(self.bits, n * 4) };
        let mut out = Vec::with_capacity(n * 4);
        for p in px.chunks_exact(4) {
            let a = p[3] as u32;
            let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
            out.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), p[3]]);
        }
        out
    }

    /// Copies the surface into a window's device context (for opaque windows).
    pub fn blit(&self, dc: HDC, x: i32, y: i32) {
        unsafe {
            let _ = BitBlt(dc, x, y, self.width, self.height, Some(self.dc), 0, 0, SRCCOPY);
        }
    }
}

/// Drawing operations on a render target, in DIPs.
pub struct Painter {
    rt: ID2D1RenderTarget,
}

impl Painter {
    fn brush(&self, c: Rgba) -> ID2D1SolidColorBrush {
        unsafe { self.rt.CreateSolidColorBrush(&c.d2d(), None).expect("brush") }
    }

    pub fn path(&self, cmds: &[PathCmd], cx: f32, cy: f32) -> Option<ID2D1PathGeometry> {
        // Core geometry has y pointing up; the screen has y pointing down.
        let p = |pt: satsuma_core::geometry::Point| D2D_POINT_2F { X: cx + pt.x as f32, Y: cy - pt.y as f32 };
        unsafe {
            let geo = FACTORIES.with(|f| f.d2d.CreatePathGeometry()).ok()?;
            let sink = geo.Open().ok()?;
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            let mut open = false;
            for cmd in cmds {
                match *cmd {
                    PathCmd::MoveTo(pt) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        sink.BeginFigure(p(pt), D2D1_FIGURE_BEGIN_FILLED);
                        open = true;
                    }
                    PathCmd::LineTo(pt) => sink.AddLine(p(pt)),
                    PathCmd::CubicTo { c1, c2, to } => sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: p(c1), point2: p(c2), point3: p(to) }),
                    PathCmd::Close => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                            open = false;
                        }
                    }
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
            sink.Close().ok()?;
            Some(geo)
        }
    }

    pub fn fill_geometry(&self, geo: &ID2D1PathGeometry, c: Rgba) {
        unsafe { self.rt.FillGeometry(geo, &self.brush(c), None) }
    }

    pub fn stroke_geometry(&self, geo: &ID2D1PathGeometry, c: Rgba, width: f32) {
        unsafe { self.rt.DrawGeometry(geo, &self.brush(c), width, None) }
    }

    pub fn fill_circle(&self, x: f32, y: f32, r: f32, c: Rgba) {
        let e = D2D1_ELLIPSE { point: D2D_POINT_2F { X: x, Y: y }, radiusX: r, radiusY: r };
        unsafe { self.rt.FillEllipse(&e, &self.brush(c)) }
    }

    pub fn stroke_circle(&self, x: f32, y: f32, r: f32, c: Rgba, width: f32) {
        let e = D2D1_ELLIPSE { point: D2D_POINT_2F { X: x, Y: y }, radiusX: r, radiusY: r };
        unsafe { self.rt.DrawEllipse(&e, &self.brush(c), width, None) }
    }

    pub fn fill_round_rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, radiusX: r, radiusY: r };
        unsafe { self.rt.FillRoundedRectangle(&rr, &self.brush(c)) }
    }

    /// A rounded rectangle filled with a linear gradient from `start` to `end` (evenly spaced stops).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_round_rect_gradient(&self, x: f32, y: f32, w: f32, h: f32, r: f32, stops: &[Rgba], start: (f32, f32), end: (f32, f32)) {
        let n = stops.len().max(2) - 1;
        let gs: Vec<D2D1_GRADIENT_STOP> = stops.iter().enumerate().map(|(i, c)| D2D1_GRADIENT_STOP { position: i as f32 / n as f32, color: c.d2d() }).collect();
        let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, radiusX: r, radiusY: r };
        unsafe {
            let Ok(collection) = self.rt.CreateGradientStopCollection(&gs, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: D2D_POINT_2F { X: start.0, Y: start.1 }, endPoint: D2D_POINT_2F { X: end.0, Y: end.1 } };
            if let Ok(brush) = self.rt.CreateLinearGradientBrush(&props, None, &collection) {
                self.rt.FillRoundedRectangle(&rr, &brush);
            }
        }
    }

    pub fn stroke_round_rect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba, width: f32) {
        let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, radiusX: r, radiusY: r };
        unsafe { self.rt.DrawRoundedRectangle(&rr, &self.brush(c), width, None) }
    }

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba, width: f32) {
        unsafe { self.rt.DrawLine(D2D_POINT_2F { X: x0, Y: y0 }, D2D_POINT_2F { X: x1, Y: y1 }, &self.brush(c), width, None) }
    }

    /// Text in a rectangle. `wrap` wraps words; otherwise one line, trimmed with an ellipsis.
    pub fn text(&self, s: &str, rect: (f32, f32, f32, f32), size: f32, weight: Weight, c: Rgba, align: Align, wrap: bool) {
        self.text_in_family("Segoe UI", s, rect, size, weight, c, align, wrap);
    }

    /// Capsules melted together like liquid (30nama's pill): each piece is a capsule, and
    /// pieces closer than `reach` merge with a smooth union. Drawn into `area` with a fill
    /// and a 1 px rim, at `scale` pixels per DIP. Pieces are in DIPs.
    pub fn liquid(&self, area: (f32, f32, f32, f32), pieces: &[(f32, f32, f32, f32)], fill: Rgba, rim: Rgba, scale: f32) {
        const REACH: f32 = 11.0;
        const RIM: f32 = 1.2;
        let (w, h) = ((area.2 * scale).ceil().max(1.0) as u32, (area.3 * scale).ceil().max(1.0) as u32);
        // Signed distance to a capsule (a rounded box with fully round ends).
        let capsule = |x: f32, y: f32, r: (f32, f32, f32, f32)| {
            let rad = r.2.min(r.3) / 2.0;
            let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
            let qx = (x - cx).abs() - (r.2 / 2.0 - rad);
            let qy = (y - cy).abs() - (r.3 / 2.0 - rad);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            outside + qx.max(qy).min(0.0) - rad
        };
        let join = |a: f32, b: f32| {
            let k = ((REACH - (a - b).abs()).max(0.0)) / REACH;
            a.min(b) - k * k * REACH / 4.0
        };
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for py in 0..h {
            for px in 0..w {
                let (x, y) = (area.0 + (px as f32 + 0.5) / scale, area.1 + (py as f32 + 0.5) / scale);
                let mut d = f32::MAX;
                for p in pieces.iter().filter(|p| p.2 > 0.5 && p.3 > 0.5) {
                    let pd = capsule(x, y, *p);
                    d = if d == f32::MAX { pd } else { join(d, pd) };
                }
                if d == f32::MAX {
                    continue;
                }
                let cover = (0.5 - d * scale).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                let inner = (0.5 - (d + RIM) * scale).clamp(0.0, 1.0);
                let band = (cover - inner).max(0.0);
                let (fa, ra) = (fill.3 * inner, rim.3 * band);
                let a = fa + ra * (1.0 - fa);
                if a <= 0.0 {
                    continue;
                }
                let mix = |f: f32, r: f32| ((f * fa + r * ra * (1.0 - fa)) / a * 255.0).round().clamp(0.0, 255.0) as u8;
                let i = ((py * w + px) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&[mix(fill.0, rim.0), mix(fill.1, rim.1), mix(fill.2, rim.2), (a * 255.0).round() as u8]);
            }
        }
        self.image(&rgba, w, h, area);
    }

    /// Width of one line of text in DIPs.
    pub fn text_width(&self, s: &str, size: f32, weight: Weight) -> f32 {
        let Some(format) = text_format("Segoe UI", size, weight, false) else { return 0.0 };
        let text: Vec<u16> = s.encode_utf16().collect();
        FACTORIES.with(|f| unsafe {
            f.dwrite
                .CreateTextLayout(&text, &format, 10_000.0, 100.0)
                .and_then(|layout| {
                    let mut m = DWRITE_TEXT_METRICS::default();
                    layout.GetMetrics(&mut m).map(|_| m.widthIncludingTrailingWhitespace)
                })
                .unwrap_or(0.0)
        })
    }

    /// One line centred on `(x, y)`.
    pub fn text_centered(&self, s: &str, x: f32, y: f32, size: f32, weight: Weight, c: Rgba) {
        self.text(s, (x - 120.0, y - size, 240.0, size * 2.0), size, weight, c, Align::Center, false);
    }

    /// An icon glyph (Segoe Fluent Icons / MDL2 Assets code point) centred on `(x, y)`.
    pub fn icon(&self, glyph: char, x: f32, y: f32, size: f32, c: Rgba) {
        let family = FACTORIES.with(|f| f.icon_family);
        self.text_in_family(family, &glyph.to_string(), (x - size, y - size, size * 2.0, size * 2.0), size, Weight::Regular, c, Align::Center, false);
    }

    #[allow(clippy::too_many_arguments)]
    fn text_in_family(&self, family: &str, s: &str, rect: (f32, f32, f32, f32), size: f32, weight: Weight, c: Rgba, align: Align, wrap: bool) {
        let Some(format) = text_format(family, size, weight, wrap) else { return };
        unsafe {
            let _ = format.SetTextAlignment(if align == Align::Center { DWRITE_TEXT_ALIGNMENT_CENTER } else { DWRITE_TEXT_ALIGNMENT_LEADING });
        }
        let text: Vec<u16> = s.encode_utf16().collect();
        let r = D2D_RECT_F { left: rect.0, top: rect.1, right: rect.0 + rect.2, bottom: rect.1 + rect.3 };
        unsafe {
            self.rt.DrawText(&text, &format, &r, &self.brush(c), D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    /// Draws a straight-RGBA image into `rect`.
    pub fn image(&self, rgba: &[u8], w: u32, h: u32, rect: (f32, f32, f32, f32)) {
        let mut pre = Vec::with_capacity(rgba.len());
        for p in rgba.chunks_exact(4) {
            let a = p[3] as u32;
            let m = |c: u8| ((c as u32 * a + 127) / 255) as u8;
            pre.extend_from_slice(&[m(p[2]), m(p[1]), m(p[0]), p[3]]);
        }
        let props = D2D1_BITMAP_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
        };
        unsafe {
            if let Ok(bmp) = self.rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(pre.as_ptr() as *const _), w * 4, &props) {
                let r = D2D_RECT_F { left: rect.0, top: rect.1, right: rect.0 + rect.2, bottom: rect.1 + rect.3 };
                self.rt.DrawBitmap(&bmp, Some(&r), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
            }
        }
    }
}

fn text_format(family: &str, size: f32, weight: Weight, wrap: bool) -> Option<IDWriteTextFormat> {
    FACTORIES.with(|f| {
        let key = (family.to_string(), (size * 100.0) as u32, weight.dwrite().0, wrap);
        if let Some(fmt) = f.formats.borrow().get(&key) {
            return Some(fmt.clone());
        }
        let fmt = unsafe {
            f.dwrite
                .CreateTextFormat(
                    &HSTRING::from(family),
                    None,
                    weight.dwrite(),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()?
        };
        unsafe {
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            if wrap {
                let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            } else {
                let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                if let Ok(sign) = f.dwrite.CreateEllipsisTrimmingSign(&fmt) {
                    let _ = fmt.SetTrimming(&trimming, &sign);
                }
            }
        }
        f.formats.borrow_mut().insert(key, fmt.clone());
        Some(fmt)
    })
}

/// Writes straight RGBA pixels as a PNG (for `render-wheel` and friends).
pub fn save_png(rgba: &[u8], w: u32, h: u32, out: &std::path::Path) -> Result<(), String> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("Bad image buffer")?;
    img.save_with_format(out, image::ImageFormat::Png).map_err(|e| e.to_string())
}

#[allow(dead_code)]
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[allow(dead_code)]
pub(crate) fn pcwstr(v: &[u16]) -> PCWSTR {
    PCWSTR(v.as_ptr())
}
