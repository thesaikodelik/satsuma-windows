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
    /// The render target lost its device (EndDraw failed); don't keep it for reuse.
    lost: std::cell::Cell<bool>,
}

thread_local! {
    /// Recently used surfaces. Making a DIB and a render target is the costly part of a
    /// frame, so animations reuse them; a few sizes cover the windows that animate.
    static POOL: std::cell::RefCell<Vec<Surface>> = const { std::cell::RefCell::new(Vec::new()) };
}

const POOL_SIZE: usize = 3;

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
            Ok(Surface { width, height, dc, bitmap, old, bits: bits as *mut u8, rt, lost: std::cell::Cell::new(false) })
        }
    }

    /// Runs `f` with a `width` × `height` surface, reusing one from an earlier frame when
    /// the size matches.
    pub fn with<R>(width: i32, height: i32, f: impl FnOnce(&Surface) -> R) -> Option<R> {
        let (width, height) = (width.max(1), height.max(1));
        let reused = POOL.with(|p| {
            let mut p = p.try_borrow_mut().ok()?;
            let i = p.iter().position(|s| s.width == width && s.height == height)?;
            Some(p.remove(i))
        });
        let surface = match reused {
            Some(s) => s,
            None => Surface::new(width, height).ok()?,
        };
        let result = f(&surface);
        if !surface.lost.get() {
            POOL.with(|p| {
                if let Ok(mut p) = p.try_borrow_mut() {
                    p.push(surface);
                    if p.len() > POOL_SIZE {
                        p.remove(0);
                    }
                }
            });
        }
        Some(result)
    }

    /// Frees the kept surfaces (when nothing is animating).
    pub fn release_pool() {
        POOL.with(|p| {
            if let Ok(mut p) = p.try_borrow_mut() {
                p.clear();
            }
        });
    }

    /// Clears to transparent and draws with `f`, scaled by `scale` (DIPs → pixels).
    pub fn draw(&self, scale: f32, f: impl FnOnce(&Painter)) {
        unsafe {
            self.rt.BeginDraw();
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            self.rt.SetTransform(&Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: 0.0, M32: 0.0 });
            let painter = Painter { rt: self.rt.clone().into() };
            f(&painter);
            if self.rt.EndDraw(None, None).is_err() {
                self.lost.set(true);
            }
            let _ = GdiFlush();
        }
    }

    /// Like [`Surface::draw`], but only inside `clip` (DIPs); the rest keeps the last frame.
    pub fn draw_clipped(&self, scale: f32, clip: (f32, f32, f32, f32), f: impl FnOnce(&Painter)) {
        unsafe {
            self.rt.BeginDraw();
            self.rt.SetTransform(&Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: 0.0, M32: 0.0 });
            let rect = D2D_RECT_F { left: clip.0, top: clip.1, right: clip.0 + clip.2, bottom: clip.1 + clip.3 };
            self.rt.PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_ALIASED);
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            let painter = Painter { rt: self.rt.clone().into() };
            f(&painter);
            self.rt.PopAxisAlignedClip();
            if self.rt.EndDraw(None, None).is_err() {
                self.lost.set(true);
            }
            let _ = GdiFlush();
        }
    }

    /// Copies part of the surface (pixels) to `dc` at the same position.
    pub fn blit_rect(&self, dc: HDC, r: RECT) {
        unsafe {
            let _ = BitBlt(dc, r.left, r.top, r.right - r.left, r.bottom - r.top, Some(self.dc), r.left, r.top, SRCCOPY);
        }
    }

    /// Whether the render target is still usable.
    pub fn is_lost(&self) -> bool {
        self.lost.get()
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

    /// Part of the circle of radius `r` around (`cx`, `cy`), from `from` to `to` (fractions
    /// of a turn, clockwise from the top), stroked with round ends.
    pub fn stroke_arc(&self, cx: f32, cy: f32, r: f32, from: f32, to: f32, c: Rgba, width: f32) {
        self.stroke_arc_ends(cx, cy, r, from, to, c, width, true);
    }

    /// [`Painter::stroke_arc`] with round or flat ends.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_arc_ends(&self, cx: f32, cy: f32, r: f32, from: f32, to: f32, c: Rgba, width: f32, round: bool) {
        if to - from <= 0.0005 {
            return;
        }
        let at = |f: f32| {
            let a = f * std::f32::consts::TAU;
            D2D_POINT_2F { X: cx + r * a.sin(), Y: cy - r * a.cos() }
        };
        unsafe {
            let Ok(geo) = FACTORIES.with(|f| f.d2d.CreatePathGeometry()) else { return };
            let Ok(sink) = geo.Open() else { return };
            sink.BeginFigure(at(from), D2D1_FIGURE_BEGIN_HOLLOW);
            // A quarter turn at most per segment, so the arc's direction is never ambiguous.
            let mut a = from;
            while a < to - 1e-4 {
                let b = (a + 0.25).min(to);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: at(b),
                    size: D2D_SIZE_F { width: r, height: r },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: D2D1_ARC_SIZE_SMALL,
                });
                a = b;
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_err() {
                return;
            }
            let cap = if round { D2D1_CAP_STYLE_ROUND } else { D2D1_CAP_STYLE_FLAT };
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: cap,
                endCap: cap,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let style = FACTORIES.with(|f| f.d2d.CreateStrokeStyle(&props, None)).ok();
            self.rt.DrawGeometry(&geo, &self.brush(c), width, style.as_ref());
        }
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

    /// A rounded rectangle filled (or, with `stroke`, outlined) with a linear gradient
    /// whose stops are `(position, colour)`.
    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_gradient(&self, x: f32, y: f32, w: f32, h: f32, r: f32, stops: &[(f32, Rgba)], start: (f32, f32), end: (f32, f32), stroke: Option<f32>) {
        let gs: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|(pos, c)| D2D1_GRADIENT_STOP { position: *pos, color: c.d2d() }).collect();
        let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }, radiusX: r, radiusY: r };
        unsafe {
            let Ok(collection) = self.rt.CreateGradientStopCollection(&gs, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) else { return };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: D2D_POINT_2F { X: start.0, Y: start.1 }, endPoint: D2D_POINT_2F { X: end.0, Y: end.1 } };
            if let Ok(brush) = self.rt.CreateLinearGradientBrush(&props, None, &collection) {
                match stroke {
                    Some(width) => self.rt.DrawRoundedRectangle(&rr, &brush, width, None),
                    None => self.rt.FillRoundedRectangle(&rr, &brush),
                }
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
    /// pieces closer than `REACH` merge with a smooth union. Pieces are in DIPs.
    pub fn liquid(&self, area: (f32, f32, f32, f32), pieces: &[(f32, f32, f32, f32)], fill: Rgba, rim: Rgba, scale: f32) {
        let pieces: Vec<_> = pieces.iter().copied().filter(|p| p.2 > 0.5 && p.3 > 0.5).collect();
        self.sdf_fill(area, scale, fill, rim, &|x, y| {
            pieces.iter().map(|p| capsule_sdf(x, y, *p)).reduce(smooth_union).unwrap_or(f32::MAX)
        });
    }

    /// Fills the shape `sdf` describes (signed distance in DIPs, negative inside) within
    /// `area`, anti-aliased, with a 1.2 DIP rim, rendered at `scale` pixels per DIP.
    pub fn sdf_fill(&self, area: (f32, f32, f32, f32), scale: f32, fill: Rgba, rim: Rgba, sdf: &dyn Fn(f32, f32) -> f32) {
        self.sdf_fill_lit(area, scale, &|_| (fill, rim), sdf);
    }

    /// Like [`Painter::sdf_fill`], with the fill and rim colours given per DIP row (`y`).
    pub fn sdf_fill_lit(&self, area: (f32, f32, f32, f32), scale: f32, colors: &dyn Fn(f32) -> (Rgba, Rgba), sdf: &dyn Fn(f32, f32) -> f32) {
        const RIM: f32 = 1.2;
        let (w, h) = ((area.2 * scale).ceil().max(1.0) as u32, (area.3 * scale).ceil().max(1.0) as u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for py in 0..h {
            let row_y = area.1 + (py as f32 + 0.5) / scale;
            let (fill, rim) = colors(row_y);
            for px in 0..w {
                let (x, y) = (area.0 + (px as f32 + 0.5) / scale, row_y);
                let d = sdf(x, y);
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

    /// Fills a rounded rectangle (a circle when `r` is half its size) with an image stretched
    /// over `rect` — used to lay the blurred backdrop exactly inside a glass shape.
    pub fn fill_round_rect_image(&self, rect: (f32, f32, f32, f32), r: f32, image_rect: (f32, f32, f32, f32), rgba: &[u8], w: u32, h: u32) {
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
            let Ok(bmp) = self.rt.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(pre.as_ptr() as *const _), w * 4, &props) else { return };
            let bp = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            let Ok(brush) = self.rt.CreateBitmapBrush(&bmp, Some(&bp), None) else { return };
            brush.SetTransform(&Matrix3x2 { M11: image_rect.2 / w as f32, M12: 0.0, M21: 0.0, M22: image_rect.3 / h as f32, M31: image_rect.0, M32: image_rect.1 });
            let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: rect.0, top: rect.1, right: rect.0 + rect.2, bottom: rect.1 + rect.3 }, radiusX: r, radiusY: r };
            self.rt.FillRoundedRectangle(&rr, &brush);
        }
    }

    /// Draws `f` shifted by `(dx, dy)` DIPs.
    pub fn translated(&self, dx: f32, dy: f32, f: &dyn Fn(&Painter)) {
        unsafe {
            let mut m = Matrix3x2::default();
            self.rt.GetTransform(&mut m);
            let moved = Matrix3x2 { M31: m.M31 + dx * m.M11, M32: m.M32 + dy * m.M22, ..m };
            self.rt.SetTransform(&moved);
            f(self);
            self.rt.SetTransform(&m);
        }
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

/// How far apart two liquid pieces can be and still melt together (30nama's `Reach`).
pub const REACH: f32 = 11.0;

/// Smooth minimum of two signed distances: shapes closer than `REACH` merge.
pub fn smooth_union(a: f32, b: f32) -> f32 {
    let k = ((REACH - (a - b).abs()).max(0.0)) / REACH;
    a.min(b) - k * k * REACH / 4.0
}

/// Signed distance to a capsule (a rounded box with fully round ends).
pub fn capsule_sdf(x: f32, y: f32, r: (f32, f32, f32, f32)) -> f32 {
    let rad = r.2.min(r.3) / 2.0;
    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    let qx = (x - cx).abs() - (r.2 / 2.0 - rad);
    let qy = (y - cy).abs() - (r.3 / 2.0 - rad);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - rad
}

/// The drag image Windows shows under the pointer during a drag (class `SysDragImage`).
fn drag_images() -> Vec<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, IsWindowVisible};
    unsafe extern "system" fn each(hwnd: HWND, list: windows::Win32::Foundation::LPARAM) -> windows::core::BOOL {
        let mut class = [0u16; 32];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        if IsWindowVisible(hwnd).as_bool() && String::from_utf16_lossy(&class[..n]) == "SysDragImage" {
            (*(list.0 as *mut Vec<HWND>)).push(hwnd);
        }
        true.into()
    }
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), windows::Win32::Foundation::LPARAM(&mut list as *mut _ as isize));
    }
    list
}

/// What is on screen in a rectangle (pixels), as straight RGBA, without the drag image
/// (which would otherwise be frozen into the glass).
pub fn capture_screen(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNA};
    let hidden = drag_images();
    for hwnd in &hidden {
        unsafe {
            let _ = ShowWindow(*hwnd, SW_HIDE);
        }
    }
    if !hidden.is_empty() {
        // Let the compositor present a frame without it.
        unsafe {
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
        }
    }
    let pixels = capture_rect(x, y, w, h);
    for hwnd in &hidden {
        unsafe {
            let _ = ShowWindow(*hwnd, SW_SHOWNA);
        }
    }
    pixels
}

fn capture_rect(x: i32, y: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    if w <= 0 || h <= 0 {
        return None;
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY).is_ok();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let rows = GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if !ok || rows == 0 {
            return None;
        }
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        Some(buf)
    }
}

/// A Gaussian-like blur (three box passes) of RGBA pixels, with a little extra saturation
/// so glass looks lively rather than grey.
pub fn blur_rgba(px: &mut [u8], w: usize, h: usize, sigma: f32) {
    let ideal = ((12.0 * sigma * sigma / 3.0) + 1.0).sqrt();
    let r = (((ideal - 1.0) / 2.0).floor().max(1.0)) as usize;
    let mut a: Vec<[f32; 3]> = px.chunks_exact(4).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let mut b = a.clone();
    let pass = |src: &[[f32; 3]], dst: &mut [[f32; 3]], horizontal: bool| {
        let (len, lines) = if horizontal { (w, h) } else { (h, w) };
        let idx = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
        let norm = 1.0 / (2 * r + 1) as f32;
        for line in 0..lines {
            let at = |i: isize| src[idx(line, i.clamp(0, len as isize - 1) as usize)];
            let mut acc = [0f32; 3];
            for i in -(r as isize)..=(r as isize) {
                let p = at(i);
                acc = [acc[0] + p[0], acc[1] + p[1], acc[2] + p[2]];
            }
            for i in 0..len as isize {
                dst[idx(line, i as usize)] = [acc[0] * norm, acc[1] * norm, acc[2] * norm];
                let (add, sub) = (at(i + r as isize + 1), at(i - r as isize));
                acc = [acc[0] + add[0] - sub[0], acc[1] + add[1] - sub[1], acc[2] + add[2] - sub[2]];
            }
        }
    };
    for _ in 0..3 {
        pass(&a, &mut b, true);
        pass(&b, &mut a, false);
    }
    for (p, c) in px.chunks_exact_mut(4).zip(a) {
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        for k in 0..3 {
            p[k] = (l + (c[k] - l) * 1.25).round().clamp(0.0, 255.0) as u8;
        }
    }
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
