//! The editor windows: Crop Image, Add Background and Edit Photo, with live previews.
//! Same layout, presets and behaviour as the macOS editors (`editor.rs`); the controls are
//! drawn with Direct2D so they follow the light or dark theme, and the exact-size fields are
//! native edit controls.

use super::events::{self, UiEvent};
use super::gfx::{Align, Painter, Rgba, Surface, Weight};
use super::theme::{Palette, ACCENT};
use super::window::{self, Handler};
use super::system;
use satsuma_core::edit::{
    drag, fit_aspect, hit_handle, ratio_value, set_size, Adjustments, Background, CropSpec, FrameSpec, Handle, Rect, Section, ADJUSTMENTS,
    CROP_ASPECTS, FRAME_RATIOS, GRADIENTS, SOLIDS,
};
use satsuma_core::Tool;
use satsuma_engine::imaging::{self, Image};
use satsuma_engine::{photo, Operation};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Content width in DIPs, as on macOS.
const W: f32 = 460.0;
/// macOS heights include a 32-point title area that Windows draws itself.
const TITLE: f32 = 32.0;
const GRAB: f32 = 12.0;
const SWATCH: f32 = 26.0;
const SWATCH_GAP: f32 = 8.0;
const ADJUST_ROWS_TOP: f32 = 58.0 + 7.0 * 30.0;

type R = (f32, f32, f32, f32);

fn contains(r: R, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

/// Fits `size` into `bounds`, centred, never enlarging past the bounds.
fn fit(size: (f64, f64), b: R) -> R {
    let s = (b.2 as f64 / size.0).min(b.3 as f64 / size.1);
    let (w, h) = ((size.0 * s) as f32, (size.1 * s) as f32);
    (b.0 + (b.2 - w) / 2.0, b.1 + (b.3 - h) / 2.0, w, h)
}

fn signed(v: f64) -> String {
    if v > 0.0 {
        format!("+{}", v as i64)
    } else if v < 0.0 {
        format!("−{}", (-v) as i64)
    } else {
        "0".into()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Id {
    Reset,
    Apply,
    ShowOriginal,
    Segment(usize),
    Slider(usize),
    Gradient(usize),
    Solid(usize),
    ImageButton,
    ColorButton,
    CropArea,
}

struct CropState {
    image: Image,
    size: (f64, f64),
    rect: Rect,
    aspect_index: usize,
    drag: Option<(Handle, Rect, (f32, f32))>,
    fields: Option<(HWND, HWND)>,
}

struct FrameState {
    full_size: (f64, f64),
    preview_src: Image,
    preview_scale: f64,
    background_image: Option<Image>,
    spec: FrameSpec,
    limits: (f64, f64, f64),
    ratio_index: usize,
    preview: Option<Image>,
}

struct AdjustState {
    preview_src: Image,
    adjustments: Adjustments,
    section: usize,
    show_original: bool,
    preview: Option<Image>,
}

enum Kind {
    Crop(CropState),
    Frame(FrameState),
    Adjust(AdjustState),
}

struct Editor {
    kind: Kind,
    files: Vec<PathBuf>,
    /// Client height in DIPs.
    height: f32,
    pressed: Option<Id>,
    slider_drag: Option<usize>,
}

/// macOS rect (y up, in a window `h` tall including the title area) → client DIPs, y down.
fn mac(h: f32, x: f32, y: f32, w: f32, hh: f32) -> R {
    (x, h - TITLE - y - hh, w, hh)
}

impl Editor {
    fn mh(&self) -> f32 {
        self.height + TITLE
    }

    fn title(&self) -> &'static str {
        match self.kind {
            Kind::Crop(_) => "Crop Image",
            Kind::Frame(_) => "Add Background",
            Kind::Adjust(_) => "Edit Photo",
        }
    }

    /// Every interactive widget and its rectangle.
    fn widgets(&self) -> Vec<(Id, R)> {
        let h = self.mh();
        let mut v = Vec::new();
        match &self.kind {
            Kind::Crop(_) => {
                v.push((Id::Reset, mac(h, 16.0, 14.0, 96.0, 30.0)));
                v.push((Id::Apply, mac(h, W - 126.0, 14.0, 110.0, 30.0)));
                let seg = mac(h, 108.0, 96.0, W - 124.0, 26.0);
                let n = CROP_ASPECTS.len();
                for i in 0..n {
                    v.push((Id::Segment(i), (seg.0 + seg.2 * i as f32 / n as f32, seg.1, seg.2 / n as f32, seg.3)));
                }
                v.push((Id::CropArea, mac(h, 12.0, 130.0, W - 24.0, h - 162.0)));
            }
            Kind::Frame(_) => {
                v.push((Id::Reset, mac(h, 16.0, 14.0, 96.0, 30.0)));
                v.push((Id::Apply, mac(h, W - 206.0, 14.0, 190.0, 30.0)));
                let seg = mac(h, W - 246.0, 58.0, 230.0, 26.0);
                let n = FRAME_RATIOS.len();
                for i in 0..n {
                    v.push((Id::Segment(i), (seg.0 + seg.2 * i as f32 / n as f32, seg.1, seg.2 / n as f32, seg.3)));
                }
                // Display order top to bottom: Padding (156), Corners (126), Shadow (96).
                for (i, y) in [(0, 156.0), (1, 126.0), (2, 96.0)] {
                    v.push((Id::Slider(i), mac(h, 100.0, y, W - 190.0, 24.0)));
                }
                for i in 0..GRADIENTS.len() {
                    v.push((Id::Gradient(i), mac(h, 16.0 + i as f32 * (SWATCH + SWATCH_GAP), 216.0, SWATCH, SWATCH)));
                }
                for i in 0..SOLIDS.len() {
                    v.push((Id::Solid(i), mac(h, 16.0 + i as f32 * (SWATCH + SWATCH_GAP), 186.0, SWATCH, SWATCH)));
                }
                v.push((Id::ImageButton, mac(h, W - 100.0, 213.0, 84.0, 30.0)));
                v.push((Id::ColorButton, mac(h, W - 60.0, 186.0, 44.0, 26.0)));
            }
            Kind::Adjust(a) => {
                v.push((Id::Reset, mac(h, 16.0, 14.0, 90.0, 30.0)));
                v.push((Id::ShowOriginal, mac(h, 112.0, 14.0, 130.0, 30.0)));
                v.push((Id::Apply, mac(h, W - 186.0, 14.0, 170.0, 30.0)));
                let seg = mac(h, (W - 320.0) / 2.0, 306.0, 320.0, 26.0);
                for i in 0..4 {
                    v.push((Id::Segment(i), (seg.0 + seg.2 * i as f32 / 4.0, seg.1, seg.2 / 4.0, seg.3)));
                }
                for (k, i) in Adjustments::section(Section::ALL[a.section]).into_iter().enumerate() {
                    let y = ADJUST_ROWS_TOP - k as f32 * 30.0;
                    v.push((Id::Slider(i), mac(h, 120.0, y, W - 200.0, 24.0)));
                }
            }
        }
        v
    }

    fn hit(&self, x: f32, y: f32) -> Option<Id> {
        self.widgets().into_iter().find(|(_, r)| contains(*r, x, y)).map(|(id, _)| id)
    }

    fn rect_of(&self, id: Id) -> Option<R> {
        self.widgets().into_iter().find(|(i, _)| *i == id).map(|(_, r)| r)
    }

    /// Slider range and value.
    fn slider(&self, i: usize) -> (f64, f64, f64) {
        match &self.kind {
            Kind::Frame(f) => match i {
                0 => (0.0, f.limits.0, f.spec.padding),
                1 => (0.0, f.limits.1, f.spec.corners),
                _ => (0.0, f.limits.2, f.spec.shadow),
            },
            Kind::Adjust(a) => (ADJUSTMENTS[i].min, ADJUSTMENTS[i].max, a.adjustments.0[i]),
            Kind::Crop(_) => (0.0, 1.0, 0.0),
        }
    }

    fn set_slider_from_x(&mut self, i: usize, x: f32) {
        let Some(r) = self.rect_of(Id::Slider(i)) else { return };
        let (min, max, _) = self.slider(i);
        let t = ((x - r.0 - 8.0) / (r.2 - 16.0)).clamp(0.0, 1.0) as f64;
        let v = (min + t * (max - min)).round();
        match &mut self.kind {
            Kind::Frame(f) => {
                match i {
                    0 => f.spec.padding = v,
                    1 => f.spec.corners = v,
                    _ => f.spec.shadow = v,
                }
                self.render_frame_preview();
            }
            Kind::Adjust(a) => {
                a.adjustments.set(i, v);
                a.show_original = false;
                self.render_adjust_preview();
            }
            Kind::Crop(_) => {}
        }
    }

    // ---- crop geometry

    fn crop_frame(&self, c: &CropState) -> R {
        let area = self.rect_of(Id::CropArea).unwrap_or((0.0, 0.0, 1.0, 1.0));
        fit(c.size, (area.0 + GRAB, area.1 + GRAB, area.2 - 2.0 * GRAB, area.3 - 2.0 * GRAB))
    }

    fn set_crop(&mut self, r: Rect) {
        if let Kind::Crop(c) = &mut self.kind {
            c.rect = r;
            if let Some((wf, hf)) = c.fields {
                set_text(wf, &format!("{}", r.w.round() as i64));
                set_text(hf, &format!("{}", r.h.round() as i64));
            }
        }
    }

    fn crop_aspect_changed(&mut self, i: usize) {
        let Kind::Crop(c) = &mut self.kind else { return };
        c.aspect_index = i;
        if let Some(a) = CROP_ASPECTS.get(i).and_then(|a| a.1).map(ratio_value) {
            let r = fit_aspect(c.rect, a, c.size);
            self.set_crop(r);
        }
    }

    fn crop_size_edited(&mut self) {
        let Kind::Crop(c) = &mut self.kind else { return };
        let Some((wf, hf)) = c.fields else { return };
        let (w, h) = (get_number(wf), get_number(hf));
        let (Some(w), Some(h)) = (w, h) else { return };
        // Typed sizes override the aspect preset.
        c.aspect_index = 0;
        let r = set_size(c.rect, w.max(1) as f64, h.max(1) as f64, c.size);
        self.set_crop(r);
    }

    // ---- previews

    fn render_frame_preview(&mut self) {
        let Kind::Frame(f) = &mut self.kind else { return };
        let s = f.preview_scale;
        let scaled = FrameSpec { padding: f.spec.padding * s, corners: f.spec.corners * s, shadow: f.spec.shadow * s, ..f.spec.clone() };
        f.preview = imaging::frame(&f.preview_src, &scaled, f.background_image.as_deref(), 1.0).ok();
    }

    fn render_adjust_preview(&mut self) {
        let Kind::Adjust(a) = &mut self.kind else { return };
        a.preview = if a.show_original { None } else { photo::apply(&a.preview_src, &a.adjustments).ok() };
    }

    fn set_background(&mut self, bg: Background) {
        if let Kind::Frame(f) = &mut self.kind {
            f.spec.background = bg;
        }
        self.render_frame_preview();
    }

    fn reset(&mut self) {
        match &mut self.kind {
            Kind::Crop(c) => {
                c.aspect_index = 0;
                let size = c.size;
                self.set_crop(Rect::new(0.0, 0.0, size.0, size.1));
            }
            Kind::Frame(f) => {
                f.spec = FrameSpec::defaults_for(f.full_size);
                f.background_image = None;
                f.ratio_index = 0;
                self.render_frame_preview();
            }
            Kind::Adjust(a) => {
                a.adjustments = Adjustments::default();
                a.show_original = false;
                self.render_adjust_preview();
            }
        }
    }

    fn operation(&self) -> Operation {
        match &self.kind {
            Kind::Crop(c) => Operation::Crop(CropSpec::from_pixels(c.rect.rounded(c.size), c.size, None)),
            Kind::Frame(f) => Operation::Frame(f.spec.clone()),
            Kind::Adjust(a) => Operation::Adjust(a.adjustments),
        }
    }

    fn select_segment(&mut self, i: usize) {
        match &mut self.kind {
            Kind::Crop(_) => self.crop_aspect_changed(i),
            Kind::Frame(f) => {
                f.ratio_index = i;
                f.spec.ratio = FRAME_RATIOS.get(i).and_then(|r| r.1);
                self.render_frame_preview();
            }
            Kind::Adjust(a) => a.section = i.min(3),
        }
    }

    // ---- painting

    fn paint(&self, p: &Painter, pal: &Palette) {
        p.fill_round_rect(0.0, 0.0, W, self.height, 0.0, opaque(pal.panel));
        let h = self.mh();
        let label = |text: &str, r: R, align: Align| p.text(text, r, 12.0, Weight::Regular, pal.label, align, false);
        match &self.kind {
            Kind::Crop(c) => {
                label("W", mac(h, 16.0, 63.0, 18.0, 18.0), Align::Leading);
                field(p, pal, mac(h, 36.0, 60.0, 80.0, 24.0), &format!("{}", c.rect.w.round() as i64));
                label("×", mac(h, 120.0, 63.0, 16.0, 18.0), Align::Center);
                field(p, pal, mac(h, 140.0, 60.0, 80.0, 24.0), &format!("{}", c.rect.h.round() as i64));
                label("px", mac(h, 226.0, 63.0, 24.0, 18.0), Align::Leading);
                label("Aspect ratio", mac(h, 16.0, 101.0, 90.0, 18.0), Align::Leading);
                let names: Vec<&str> = CROP_ASPECTS.iter().map(|a| a.0).collect();
                self.paint_segments(p, pal, &names, c.aspect_index);
                self.paint_crop(p, c);
            }
            Kind::Frame(f) => {
                label("Ratio", mac(h, 16.0, 63.0, 90.0, 18.0), Align::Leading);
                let names: Vec<&str> = FRAME_RATIOS.iter().map(|r| r.0).collect();
                self.paint_segments(p, pal, &names, f.ratio_index);
                for (i, name, y) in [(0, "Padding", 156.0), (1, "Corners", 126.0), (2, "Shadow", 96.0)] {
                    label(name, mac(h, 16.0, y + 3.0, 80.0, 18.0), Align::Leading);
                    self.paint_slider(p, pal, i);
                    let (_, _, v) = self.slider(i);
                    let r = mac(h, W - 84.0, y + 3.0, 68.0, 18.0);
                    p.text(&format!("{} px", v as i64), r, 12.0, Weight::Regular, pal.secondary, Align::Leading, false);
                }
                label("Background", mac(h, 16.0, 250.0, 120.0, 18.0), Align::Leading);
                for (i, g) in GRADIENTS.iter().enumerate() {
                    let r = self.rect_of(Id::Gradient(i)).unwrap();
                    let stops: Vec<Rgba> = g.stops.iter().map(|s| Rgba::hex(((s.0 as u32) << 16) | ((s.1 as u32) << 8) | s.2 as u32, 1.0)).collect();
                    // −45°: top-left to bottom-right.
                    p.fill_round_rect_gradient(r.0, r.1, r.2, r.3, 7.0, &stops, (r.0, r.1), (r.0 + r.2, r.1 + r.3));
                    p.stroke_round_rect(r.0, r.1, r.2, r.3, 7.0, pal.separator, 0.5);
                    if matches!(f.spec.background, Background::Gradient(k) if k == i) {
                        p.stroke_round_rect(r.0 + 1.5, r.1 + 1.5, r.2 - 3.0, r.3 - 3.0, 6.0, ACCENT, 3.0);
                    }
                }
                for (i, (_, rgb)) in SOLIDS.iter().enumerate() {
                    let r = self.rect_of(Id::Solid(i)).unwrap();
                    p.fill_round_rect(r.0, r.1, r.2, r.3, 7.0, Rgba::hex(((rgb.0 as u32) << 16) | ((rgb.1 as u32) << 8) | rgb.2 as u32, 1.0));
                    p.stroke_round_rect(r.0, r.1, r.2, r.3, 7.0, pal.separator, 0.5);
                    if matches!(f.spec.background, Background::Solid { r: a, g: b, b: c } if (a, b, c) == *rgb) {
                        p.stroke_round_rect(r.0 + 1.5, r.1 + 1.5, r.2 - 3.0, r.3 - 3.0, 6.0, ACCENT, 3.0);
                    }
                }
                self.paint_button(p, pal, Id::ImageButton, "Image…", false, matches!(f.spec.background, Background::Image(_)));
                // Custom colour: the current colour when it isn't a preset, else a colour wheel glyph.
                let r = self.rect_of(Id::ColorButton).unwrap();
                let custom = match f.spec.background {
                    Background::Solid { r, g, b } if !SOLIDS.iter().any(|s| s.1 == (r, g, b)) => Some((r, g, b)),
                    _ => None,
                };
                p.fill_round_rect(r.0, r.1, r.2, r.3, 6.0, pal.control);
                p.stroke_round_rect(r.0, r.1, r.2, r.3, 6.0, if custom.is_some() { ACCENT } else { pal.separator }, if custom.is_some() { 2.0 } else { 0.5 });
                match custom {
                    Some(c) => p.fill_round_rect(r.0 + 5.0, r.1 + 5.0, r.2 - 10.0, r.3 - 10.0, 4.0, Rgba::hex(((c.0 as u32) << 16) | ((c.1 as u32) << 8) | c.2 as u32, 1.0)),
                    None => p.icon(char::from_u32(0xE790).unwrap(), r.0 + r.2 / 2.0, r.1 + r.3 / 2.0, 14.0, pal.label),
                }
                let area = mac(h, 16.0, 282.0, W - 32.0, h - 318.0);
                if let Some(img) = &f.preview {
                    draw_image(p, img, area);
                }
            }
            Kind::Adjust(a) => {
                let names: Vec<&str> = Section::ALL.iter().map(|s| s.name()).collect();
                self.paint_segments(p, pal, &names, a.section);
                for (k, i) in Adjustments::section(Section::ALL[a.section]).into_iter().enumerate() {
                    let y = ADJUST_ROWS_TOP - k as f32 * 30.0;
                    label(ADJUSTMENTS[i].label, mac(h, 16.0, y + 3.0, 100.0, 18.0), Align::Leading);
                    self.paint_slider(p, pal, i);
                    let r = mac(h, W - 72.0, y + 3.0, 56.0, 18.0);
                    p.text(&signed(a.adjustments.0[i]), r, 12.0, Weight::Regular, pal.secondary, Align::Leading, false);
                }
                let area = mac(h, 16.0, 344.0, W - 32.0, h - 380.0);
                match (&a.preview, a.show_original) {
                    (Some(img), false) => draw_image(p, img, area),
                    _ => draw_image(p, &a.preview_src, area),
                }
            }
        }
        let (apply, toggle) = match &self.kind {
            Kind::Crop(_) => ("Apply", None),
            Kind::Frame(_) => ("Save with Background", None),
            Kind::Adjust(a) => ("Save Edited Copy", Some(a.show_original)),
        };
        self.paint_button(p, pal, Id::Reset, "Reset", false, false);
        if let Some(on) = toggle {
            self.paint_button(p, pal, Id::ShowOriginal, "Show Original", false, on);
        }
        self.paint_button(p, pal, Id::Apply, apply, true, false);
    }

    fn paint_button(&self, p: &Painter, pal: &Palette, id: Id, title: &str, primary: bool, on: bool) {
        let Some(r) = self.rect_of(id) else { return };
        let pressed = self.pressed == Some(id);
        if primary {
            p.fill_round_rect(r.0, r.1, r.2, r.3, 6.0, if pressed { Rgba(0.85, 0.36, 0.07, 1.0) } else { ACCENT });
            p.text(title, r, 12.5, Weight::Semibold, Rgba::WHITE, Align::Center, false);
        } else {
            let fill = if on { ACCENT.alpha(0.22) } else if pressed { pal.separator } else { pal.control };
            p.fill_round_rect(r.0, r.1, r.2, r.3, 6.0, fill);
            p.stroke_round_rect(r.0 + 0.25, r.1 + 0.25, r.2 - 0.5, r.3 - 0.5, 6.0, if on { ACCENT } else { pal.separator }, if on { 1.0 } else { 0.5 });
            p.text(title, r, 12.5, Weight::Regular, pal.label, Align::Center, false);
        }
    }

    fn paint_segments(&self, p: &Painter, pal: &Palette, names: &[&str], selected: usize) {
        let Some(first) = self.rect_of(Id::Segment(0)) else { return };
        let total = first.2 * names.len() as f32;
        p.fill_round_rect(first.0, first.1, total, first.3, 7.0, pal.separator.alpha(0.6));
        for (i, name) in names.iter().enumerate() {
            let r = (first.0 + first.2 * i as f32, first.1, first.2, first.3);
            if i == selected {
                p.fill_round_rect(r.0 + 2.0, r.1 + 2.0, r.2 - 4.0, r.3 - 4.0, 5.0, pal.control);
                p.stroke_round_rect(r.0 + 2.0, r.1 + 2.0, r.2 - 4.0, r.3 - 4.0, 5.0, pal.separator, 0.5);
            }
            p.text(name, r, 12.0, if i == selected { Weight::Semibold } else { Weight::Regular }, pal.label, Align::Center, false);
        }
    }

    fn paint_slider(&self, p: &Painter, pal: &Palette, i: usize) {
        let Some(r) = self.rect_of(Id::Slider(i)) else { return };
        let (min, max, v) = self.slider(i);
        let (x0, w) = (r.0 + 8.0, r.2 - 16.0);
        let cy = r.1 + r.3 / 2.0;
        let t = if max > min { ((v - min) / (max - min)) as f32 } else { 0.0 };
        p.fill_round_rect(x0, cy - 2.0, w, 4.0, 2.0, pal.separator.alpha(1.6));
        // Bipolar sliders fill from the centre; one-sided ones from the left.
        let from = if min < 0.0 { ((0.0 - min) / (max - min)) as f32 } else { 0.0 };
        let (a, b) = (from.min(t), from.max(t));
        p.fill_round_rect(x0 + w * a, cy - 2.0, w * (b - a), 4.0, 2.0, ACCENT);
        let kx = x0 + w * t;
        p.fill_circle(kx, cy, 8.0, Rgba(0.0, 0.0, 0.0, 0.12));
        p.fill_circle(kx, cy, 7.5, if pal.dark { Rgba::hex(0x4a4a4a, 1.0) } else { Rgba::WHITE });
        p.stroke_circle(kx, cy, 7.5, pal.separator, 0.5);
        p.fill_circle(kx, cy, if self.slider_drag == Some(i) { 4.0 } else { 3.0 }, ACCENT);
    }

    fn paint_crop(&self, p: &Painter, c: &CropState) {
        let f = self.crop_frame(c);
        draw_image(p, &c.image, f);
        let s = f.2 as f64 / c.size.0;
        let cr = (f.0 + (c.rect.x * s) as f32, f.1 + (c.rect.y * s) as f32, (c.rect.w * s) as f32, (c.rect.h * s) as f32);
        // Dim everything outside the crop.
        let shade = Rgba(0.0, 0.0, 0.0, 0.5);
        p.fill_round_rect(f.0, f.1, f.2, cr.1 - f.1, 0.0, shade);
        p.fill_round_rect(f.0, cr.1 + cr.3, f.2, f.1 + f.3 - cr.1 - cr.3, 0.0, shade);
        p.fill_round_rect(f.0, cr.1, cr.0 - f.0, cr.3, 0.0, shade);
        p.fill_round_rect(cr.0 + cr.2, cr.1, f.0 + f.2 - cr.0 - cr.2, cr.3, 0.0, shade);
        if c.drag.is_some() {
            for k in 1..3 {
                let t = k as f32 / 3.0;
                p.line(cr.0 + cr.2 * t, cr.1, cr.0 + cr.2 * t, cr.1 + cr.3, Rgba(1.0, 1.0, 1.0, 0.6), 0.5);
                p.line(cr.0, cr.1 + cr.3 * t, cr.0 + cr.2, cr.1 + cr.3 * t, Rgba(1.0, 1.0, 1.0, 0.6), 0.5);
            }
        }
        p.stroke_round_rect(cr.0, cr.1, cr.2, cr.3, 0.0, ACCENT, 2.0);
        let (x0, y0, x1, y1) = (cr.0, cr.1, cr.0 + cr.2, cr.1 + cr.3);
        let (xm, ym) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        for (x, y) in [(x0, y0), (xm, y0), (x1, y0), (x1, ym), (x1, y1), (xm, y1), (x0, y1), (x0, ym)] {
            p.fill_circle(x, y, 5.0, ACCENT);
        }
    }

    // ---- pointer

    fn pointer_down(&mut self, x: f32, y: f32) -> bool {
        let Some(id) = self.hit(x, y) else { return false };
        match id {
            Id::Segment(i) => self.select_segment(i),
            Id::Slider(i) => {
                self.slider_drag = Some(i);
                self.set_slider_from_x(i, x);
            }
            Id::Gradient(i) => self.set_background(Background::Gradient(i)),
            Id::Solid(i) => {
                let (r, g, b) = SOLIDS[i].1;
                self.set_background(Background::Solid { r, g, b });
            }
            Id::CropArea => {
                if let Kind::Crop(c) = &self.kind {
                    let f = self.crop_frame(c);
                    let s = f.2 as f64 / c.size.0;
                    let q = (((x - f.0) as f64) / s, ((y - f.1) as f64) / s);
                    let handle = hit_handle(c.rect, q, GRAB as f64 / s);
                    let start = c.rect;
                    if let Kind::Crop(c) = &mut self.kind {
                        c.drag = handle.map(|h| (h, start, (x, y)));
                    }
                }
            }
            _ => self.pressed = Some(id),
        }
        true
    }

    fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        if let Some(i) = self.slider_drag {
            self.set_slider_from_x(i, x);
            return true;
        }
        let next = if let Kind::Crop(c) = &self.kind {
            let Some((handle, start, p0)) = c.drag else { return false };
            let f = self.crop_frame(c);
            let s = f.2 as f64 / c.size.0;
            let aspect = CROP_ASPECTS.get(c.aspect_index).and_then(|a| a.1).map(ratio_value);
            Some(drag(start, handle, (x - p0.0) as f64 / s, (y - p0.1) as f64 / s, aspect, c.size, 16.0))
        } else {
            None
        };
        if let Some(r) = next {
            self.set_crop(r);
            return true;
        }
        false
    }

    /// Releases the pointer; returns the clicked button, if any.
    fn pointer_up(&mut self, x: f32, y: f32) -> Option<Id> {
        self.slider_drag = None;
        if let Kind::Crop(c) = &mut self.kind {
            c.drag = None;
        }
        let pressed = self.pressed.take()?;
        (self.hit(x, y) == Some(pressed)).then_some(pressed)
    }
}

fn opaque(c: Rgba) -> Rgba {
    Rgba(c.0, c.1, c.2, 1.0)
}

fn field(p: &Painter, pal: &Palette, r: R, text: &str) {
    p.fill_round_rect(r.0, r.1, r.2, r.3, 4.0, pal.control);
    p.stroke_round_rect(r.0 + 0.25, r.1 + 0.25, r.2 - 0.5, r.3 - 0.5, 4.0, pal.separator, 1.0);
    p.text(text, (r.0 + 6.0, r.1, r.2 - 12.0, r.3), 12.0, Weight::Regular, pal.label, Align::Leading, false);
}

fn draw_image(p: &Painter, img: &image::RgbaImage, area: R) {
    let r = fit((img.width() as f64, img.height() as f64), area);
    p.image(img.as_raw(), img.width(), img.height(), r);
}

fn set_text(hwnd: HWND, s: &str) {
    unsafe {
        let _ = SetWindowTextW(hwnd, &HSTRING::from(s));
    }
}

fn get_number(hwnd: HWND) -> Option<i64> {
    let mut buf = [0u16; 32];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n]).trim().parse().ok()
}

// ------------------------------------------------------------------ window

struct Messages {
    editor: Rc<RefCell<Editor>>,
    background: RefCell<Option<HBRUSH>>,
}

fn scale_of(hwnd: HWND) -> f32 {
    (unsafe { GetDpiForWindow(hwnd) }.max(96)) as f32 / 96.0
}

fn refresh(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn finish(hwnd: HWND, e: &Editor) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    let at = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    events::post(UiEvent::EditorApply { op: e.operation(), files: e.files.clone(), at });
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

/// Positions the native size fields over their painted boxes.
fn place_fields(hwnd: HWND, e: &Editor) {
    let Kind::Crop(c) = &e.kind else { return };
    let Some((wf, hf)) = c.fields else { return };
    let s = scale_of(hwnd);
    let h = e.mh();
    let font = field_font(s);
    for (f, x) in [(wf, 36.0), (hf, 140.0)] {
        let r = mac(h, x, 60.0, 80.0, 24.0);
        unsafe {
            let _ = SetWindowPos(f, None, ((r.0 + 5.0) * s) as i32, ((r.1 + 4.0) * s) as i32, ((r.2 - 10.0) * s) as i32, ((r.3 - 8.0) * s) as i32, SWP_NOZORDER | SWP_NOACTIVATE);
            SendMessageW(f, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        }
    }
}

fn field_font(scale: f32) -> HFONT {
    thread_local! {
        static FONTS: RefCell<Vec<(u32, HFONT)>> = const { RefCell::new(Vec::new()) };
    }
    let key = (scale * 100.0) as u32;
    FONTS.with(|f| {
        if let Some((_, font)) = f.borrow().iter().find(|(k, _)| *k == key) {
            return *font;
        }
        let font = unsafe {
            CreateFontW(-(12.0 * scale).round() as i32, 0, 0, 0, FW_NORMAL.0 as i32, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (FF_SWISS.0 | VARIABLE_PITCH.0) as u32, &HSTRING::from("Segoe UI"))
        };
        f.borrow_mut().push((key, font));
        font
    })
}

impl Handler for Messages {
    fn message(&self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let dip = |l: LPARAM| {
            let s = scale_of(hwnd);
            (window::loword(l.0) as f32 / s, window::hiword(l.0) as f32 / s)
        };
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let dc = BeginPaint(hwnd, &mut ps);
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    if let (Ok(surface), Ok(e)) = (Surface::new(r.right, r.bottom), self.editor.try_borrow()) {
                        let pal = Palette::current();
                        surface.draw(scale_of(hwnd), |p| e.paint(p, &pal));
                        surface.blit(dc, 0, 0);
                    }
                    let _ = EndPaint(hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_LBUTTONDOWN => {
                unsafe {
                    let _ = SetFocus(Some(hwnd));
                }
                let (x, y) = dip(lparam);
                if self.editor.try_borrow_mut().ok()?.pointer_down(x, y) {
                    unsafe { SetCapture(hwnd) };
                    refresh(hwnd);
                }
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = dip(lparam);
                if self.editor.try_borrow_mut().ok()?.pointer_move(x, y) {
                    refresh(hwnd);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                let (x, y) = dip(lparam);
                let clicked = self.editor.try_borrow_mut().ok()?.pointer_up(x, y);
                refresh(hwnd);
                match clicked {
                    Some(Id::Apply) => {
                        let e = self.editor.borrow();
                        finish(hwnd, &e);
                    }
                    Some(Id::Reset) => {
                        self.editor.borrow_mut().reset();
                        refresh(hwnd);
                    }
                    Some(Id::ShowOriginal) => {
                        let mut e = self.editor.borrow_mut();
                        if let Kind::Adjust(a) = &mut e.kind {
                            a.show_original = !a.show_original;
                        }
                        e.render_adjust_preview();
                        refresh(hwnd);
                    }
                    Some(Id::ImageButton) => {
                        if let Some(path) = system::choose_image() {
                            match imaging::load(&path) {
                                Ok(img) => {
                                    let mut e = self.editor.borrow_mut();
                                    if let Kind::Frame(f) = &mut e.kind {
                                        f.background_image = Some(img);
                                    }
                                    e.set_background(Background::Image(path));
                                }
                                Err(err) => system::alert("Couldn't use that image", &err.to_string()),
                            }
                            refresh(hwnd);
                        }
                    }
                    Some(Id::ColorButton) => {
                        let initial = match &self.editor.borrow().kind {
                            Kind::Frame(f) => match f.spec.background {
                                Background::Solid { r, g, b } => (r, g, b),
                                _ => (255, 255, 255),
                            },
                            _ => (255, 255, 255),
                        };
                        if let Some((r, g, b)) = system::choose_color(hwnd, initial) {
                            self.editor.borrow_mut().set_background(Background::Solid { r, g, b });
                            refresh(hwnd);
                        }
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_COMMAND => {
                // A size field lost focus: apply what was typed.
                let code = ((wparam.0 >> 16) & 0xffff) as u32;
                if code == EN_KILLFOCUS {
                    self.editor.try_borrow_mut().ok()?.crop_size_edited();
                    refresh(hwnd);
                }
                Some(LRESULT(0))
            }
            WM_CTLCOLOREDIT => {
                let pal = Palette::current();
                let to_ref = |c: Rgba| COLORREF((c.0 * 255.0) as u32 | ((c.1 * 255.0) as u32) << 8 | ((c.2 * 255.0) as u32) << 16);
                let dc = HDC(wparam.0 as *mut _);
                unsafe {
                    SetTextColor(dc, to_ref(pal.label));
                    SetBkColor(dc, to_ref(pal.control));
                }
                let mut bg = self.background.borrow_mut();
                let brush = *bg.get_or_insert_with(|| unsafe { CreateSolidBrush(to_ref(pal.control)) });
                Some(LRESULT(brush.0 as isize))
            }
            WM_KEYDOWN => {
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                if vk == VK_RETURN {
                    let e = self.editor.borrow();
                    finish(hwnd, &e);
                } else if vk == VK_ESCAPE {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                let r = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                if let Ok(e) = self.editor.try_borrow() {
                    place_fields(hwnd, &e);
                }
                refresh(hwnd);
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                apply_theme(hwnd);
                self.background.borrow_mut().take();
                refresh(hwnd);
                None
            }
            WM_DESTROY => {
                if let Some(b) = self.background.borrow_mut().take() {
                    unsafe {
                        let _ = DeleteObject(b.into());
                    }
                }
                events::post(UiEvent::WindowClosed { window: hwnd.0 as isize });
                None
            }
            _ => None,
        }
    }
}

fn apply_theme(hwnd: HWND) {
    let dark: i32 = system::is_dark() as i32;
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const i32 as *const _, 4);
    }
}

pub struct EditorWindow {
    hwnd: HWND,
    editor: Rc<RefCell<Editor>>,
}

fn preview_source(img: &Image) -> Result<(Image, f64), String> {
    let size = imaging::size(img);
    let scale = (900.0 / size.0.max(size.1)).min(1.0);
    let src = imaging::crop(img, Rect::new(0.0, 0.0, size.0, size.1), Some(((size.0 * scale).round().max(1.0) as u32, (size.1 * scale).round().max(1.0) as u32)))
        .map_err(|e| e.to_string())?;
    Ok((src, scale))
}

fn build(tool: Tool, files: Vec<PathBuf>) -> Result<Editor, String> {
    let first = files.first().ok_or("No image to edit.")?;
    let img = imaging::load(first).map_err(|e| e.to_string())?;
    let size = imaging::size(&img);
    let (kind, mac_height) = match tool {
        Tool::CropImage => (Kind::Crop(CropState { image: img, size, rect: Rect::new(0.0, 0.0, size.0, size.1), aspect_index: 0, drag: None, fields: None }), 640.0),
        Tool::EditImage => {
            let (preview_src, _) = preview_source(&img)?;
            (Kind::Adjust(AdjustState { preview_src, adjustments: Adjustments::default(), section: 0, show_original: false, preview: None }), 780.0)
        }
        _ => {
            let (preview_src, preview_scale) = preview_source(&img)?;
            let spec = FrameSpec::defaults_for(size);
            let limits = FrameSpec::limits_for(size);
            (
                Kind::Frame(FrameState { full_size: size, preview_src, preview_scale, background_image: None, spec, limits, ratio_index: 0, preview: None }),
                700.0,
            )
        }
    };
    let mut e = Editor { kind, files, height: mac_height - TITLE, pressed: None, slider_drag: None };
    e.render_frame_preview();
    Ok(e)
}

impl EditorWindow {
    /// Opens the editor for `tool` (CropImage, FrameImage or EditImage) on the first file.
    pub fn open(tool: Tool, files: Vec<PathBuf>) -> Result<EditorWindow, String> {
        let editor = Rc::new(RefCell::new(build(tool, files)?));
        let (title, height) = {
            let e = editor.borrow();
            (e.title(), e.height)
        };
        let (cx, cy) = system::cursor_pos();
        let scale = system::scale_at(cx, cy);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let ex = WS_EX_TOPMOST;
        let mut r = RECT { left: 0, top: 0, right: (W * scale).round() as i32, bottom: (height * scale).round() as i32 };
        unsafe {
            let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, (scale * 96.0) as u32);
        }
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let work = system::work_area_at(cx, cy);
        let x = (work.left + work.right - ww) / 2;
        let y = ((work.top + work.bottom - wh) / 2).max(work.top);
        let hwnd = window::create(ex, style, title, x, y, ww, wh, None, Rc::new(Messages { editor: editor.clone(), background: RefCell::new(None) }));
        apply_theme(hwnd);
        // The window may have landed on a monitor with another scale: size the client exactly.
        let s = scale_of(hwnd);
        if (s - scale).abs() > 0.01 {
            let mut r = RECT { left: 0, top: 0, right: (W * s).round() as i32, bottom: (height * s).round() as i32 };
            unsafe {
                let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, GetDpiForWindow(hwnd));
                let _ = SetWindowPos(hwnd, None, 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER);
            }
        }
        {
            let mut e = editor.borrow_mut();
            if let Kind::Crop(c) = &mut e.kind {
                let make = |text: String| unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        &HSTRING::from("EDIT"),
                        &HSTRING::from(text),
                        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((ES_NUMBER | ES_LEFT | ES_AUTOHSCROLL) as u32),
                        0,
                        0,
                        10,
                        10,
                        Some(hwnd),
                        None,
                        Some(window::instance()),
                        None,
                    )
                    .unwrap_or_default()
                };
                c.fields = Some((make(format!("{}", c.size.0 as i64)), make(format!("{}", c.size.1 as i64))));
            }
        }
        place_fields(hwnd, &editor.borrow());
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(EditorWindow { hwnd, editor })
    }

    pub fn id(&self) -> isize {
        self.hwnd.0 as isize
    }

    #[allow(dead_code)]
    pub fn operation(&self) -> Operation {
        self.editor.borrow().operation()
    }
}

/// Return in a size field applies the size; Tab moves between the fields.
pub fn translate_dialog_message(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN {
        return false;
    }
    let mut class = [0u16; 16];
    let n = unsafe { GetClassNameW(msg.hwnd, &mut class) } as usize;
    if !String::from_utf16_lossy(&class[..n]).eq_ignore_ascii_case("Edit") {
        return false;
    }
    let Ok(parent) = (unsafe { GetParent(msg.hwnd) }) else { return false };
    let vk = VIRTUAL_KEY(msg.wParam.0 as u16);
    unsafe {
        if vk == VK_RETURN {
            // Losing focus applies the size (EN_KILLFOCUS).
            let _ = SetFocus(Some(parent));
            return true;
        }
        if vk == VK_TAB {
            let next = GetNextDlgTabItem(parent, Some(msg.hwnd), GetKeyState(VK_SHIFT.0 as i32) < 0).unwrap_or_default();
            if !next.is_invalid() {
                let _ = SetFocus(Some(next));
                SendMessageW(next, windows::Win32::UI::Controls::EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            }
            return true;
        }
        if vk == VK_ESCAPE {
            let _ = DestroyWindow(parent);
            return true;
        }
    }
    false
}

/// Renders an editor to a PNG at 2× without showing it (`satsuma render-editor`).
pub fn render_png(kind: &str, files: Vec<PathBuf>, sets: &[(String, f64)], segment: Option<isize>, dark: bool, out: &std::path::Path) -> Result<(), String> {
    let tool = match kind {
        "crop" => Tool::CropImage,
        "edit" => Tool::EditImage,
        _ => Tool::FrameImage,
    };
    let mut e = build(tool, files)?;
    if let Kind::Adjust(a) = &mut e.kind {
        for (k, v) in sets {
            if let Some(i) = Adjustments::index(k) {
                a.adjustments.set(i, *v);
            }
        }
    }
    if let Some(i) = segment {
        e.select_segment(i.max(0) as usize);
    }
    e.render_adjust_preview();
    e.render_frame_preview();
    let scale = 2.0;
    let (w, h) = ((W * scale) as i32, (e.height * scale) as i32);
    let surface = Surface::new(w, h).map_err(|err| err.message())?;
    let pal = Palette::new(dark);
    surface.draw(scale, |p| e.paint(p, &pal));
    super::gfx::save_png(&surface.rgba(), w as u32, h as u32, out)
}
