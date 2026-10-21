//! Annotate Photo: arrows, boxes, ellipses, lines, highlights, text and numbered markers
//! drawn over a photo. The marks become an SVG in the photo's pixels, which the engine
//! draws over the full-size image; the preview draws the same SVG.

use super::form::{contains, Change, Field, Form, R};
use super::gfx::{Painter, Rgba};
use super::theme::Palette;
use super::timeline::{fit, Image};
use satsuma_core::Tool;
use satsuma_engine::plan::ToolOptions;
use satsuma_engine::Operation;
use std::cell::RefCell;
use std::fmt::Write;
use std::path::PathBuf;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

const KINDS: [&str; 7] = ["Arrow", "Box", "Ellipse", "Line", "Highlight", "Text", "Number"];
const COLOURS: [[u8; 3]; 7] = [[255, 59, 48], [255, 122, 26], [255, 214, 10], [52, 199, 89], [10, 132, 255], [20, 20, 20], [255, 255, 255]];
const SIZES: [&str; 3] = ["Thin", "Medium", "Thick"];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Arrow,
    Box,
    Ellipse,
    Line,
    Highlight,
    Text,
    Number,
}

impl Kind {
    fn from(i: usize) -> Kind {
        [Kind::Arrow, Kind::Box, Kind::Ellipse, Kind::Line, Kind::Highlight, Kind::Text, Kind::Number][i.min(6)]
    }
    fn placed(self) -> bool {
        matches!(self, Kind::Text | Kind::Number)
    }
}

#[derive(Clone)]
struct Mark {
    kind: Kind,
    /// Start and end, in the photo's pixels.
    a: (f64, f64),
    b: (f64, f64),
    colour: [u8; 3],
    size: usize,
    text: String,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// The marks as an SVG whose view box is the photo's pixels.
fn svg(marks: &[&Mark], w: u32, h: u32) -> String {
    let unit = w.max(h) as f64 / 260.0;
    let mut s = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"##);
    for m in marks {
        let stroke = unit * [0.7, 1.2, 2.0][m.size.min(2)];
        let c = hex(m.colour);
        let ((x0, y0), (x1, y1)) = (m.a, m.b);
        let (l, t, rw, rh) = (x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs());
        let common = format!(r#"stroke="{c}" stroke-width="{stroke:.1}" stroke-linecap="round" stroke-linejoin="round""#);
        let _ = match m.kind {
            Kind::Line => write!(s, r##"<line x1="{x0:.1}" y1="{y0:.1}" x2="{x1:.1}" y2="{y1:.1}" {common}/>"##),
            Kind::Arrow => {
                let (dx, dy) = (x1 - x0, y1 - y0);
                let len = (dx * dx + dy * dy).sqrt().max(1e-6);
                let (ux, uy) = (dx / len, dy / len);
                let head = (stroke * 4.5).min(len * 0.6);
                let (bx, by) = (x1 - ux * head, y1 - uy * head);
                let (px, py) = (-uy * head * 0.6, ux * head * 0.6);
                // The shaft stops inside the head so its round end doesn't poke through the tip.
                let (sx, sy) = (x1 - ux * head * 0.6, y1 - uy * head * 0.6);
                write!(
                    s,
                    r##"<line x1="{x0:.1}" y1="{y0:.1}" x2="{sx:.1}" y2="{sy:.1}" {common}/><polygon points="{x1:.1},{y1:.1} {:.1},{:.1} {:.1},{:.1}" fill="{c}" {common}/>"##,
                    bx + px,
                    by + py,
                    bx - px,
                    by - py
                )
            }
            Kind::Box => write!(s, r##"<rect x="{l:.1}" y="{t:.1}" width="{rw:.1}" height="{rh:.1}" rx="{:.1}" fill="none" {common}/>"##, stroke * 0.8),
            Kind::Ellipse => write!(s, r##"<ellipse cx="{:.1}" cy="{:.1}" rx="{:.1}" ry="{:.1}" fill="none" {common}/>"##, l + rw / 2.0, t + rh / 2.0, rw / 2.0, rh / 2.0),
            Kind::Highlight => write!(s, r##"<rect x="{l:.1}" y="{t:.1}" width="{rw:.1}" height="{rh:.1}" rx="{:.1}" fill="{c}" fill-opacity="0.38"/>"##, unit),
            Kind::Text => {
                let size = unit * [6.0, 9.0, 14.0][m.size.min(2)];
                let outline = if m.colour.iter().map(|v| *v as u32).sum::<u32>() > 600 { "#000000" } else { "#ffffff" };
                write!(
                    s,
                    r##"<text x="{x0:.1}" y="{:.1}" font-family="Segoe UI" font-weight="600" font-size="{size:.1}" fill="{c}" stroke="{outline}" stroke-opacity="0.55" stroke-width="{:.1}" paint-order="stroke" stroke-linejoin="round">{}</text>"##,
                    y0 + size * 0.35,
                    size * 0.12,
                    escape(&m.text)
                )
            }
            Kind::Number => {
                let r = unit * [4.0, 5.5, 8.0][m.size.min(2)];
                let ink = if m.colour.iter().map(|v| *v as u32).sum::<u32>() > 600 { "#000000" } else { "#ffffff" };
                write!(
                    s,
                    r##"<circle cx="{x0:.1}" cy="{y0:.1}" r="{r:.1}" fill="{c}" stroke="#ffffff" stroke-opacity="0.9" stroke-width="{:.1}"/><text x="{x0:.1}" y="{:.1}" text-anchor="middle" font-family="Segoe UI" font-weight="700" font-size="{:.1}" fill="{ink}">{}</text>"##,
                    r * 0.12,
                    y0 + r * 0.42,
                    r * 1.15,
                    escape(&m.text)
                )
            }
        };
    }
    s.push_str("</svg>");
    s
}

pub struct Annotate {
    files: Vec<PathBuf>,
    /// The photo scaled down for the preview, and its full size.
    photo: Image,
    size: (u32, u32),
    marks: Vec<Mark>,
    kind: usize,
    colour: usize,
    weight: usize,
    text: String,
    drawing: Option<Mark>,
    /// The last rendered overlay: its SVG, pixel size and image.
    cache: RefCell<Option<(String, (u32, u32), Image)>>,
}

impl Annotate {
    pub fn new(files: Vec<PathBuf>) -> Option<Annotate> {
        let full = satsuma_engine::imaging::load(files.first()?).ok()?;
        let size = full.dimensions();
        let k = (1400.0 / size.0.max(size.1) as f64).min(1.0);
        let small = if k < 1.0 {
            image::imageops::resize(&*full, ((size.0 as f64 * k) as u32).max(1), ((size.1 as f64 * k) as u32).max(1), image::imageops::FilterType::Triangle)
        } else {
            (*full).clone()
        };
        // Loading the fonts takes a moment; do it before the first text mark.
        std::thread::spawn(|| {
            let _ = satsuma_engine::imaging::render_svg(r##"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><text>a</text></svg>"##, 1, 1);
        });
        Some(Annotate {
            files,
            photo: Image { w: small.width(), h: small.height(), rgba: small.into_raw() },
            size,
            marks: Vec::new(),
            kind: 0,
            colour: 0,
            weight: 1,
            text: "Text".into(),
            drawing: None,
            cache: RefCell::new(None),
        })
    }

    fn picture(&self, r: R) -> R {
        fit(self.size.0, self.size.1, (r.0 + 8.0, r.1 + 8.0, r.2 - 16.0, r.3 - 16.0))
    }

    fn to_photo(&self, pic: R, x: f32, y: f32) -> (f64, f64) {
        let fx = ((x - pic.0) / pic.2).clamp(0.0, 1.0) as f64;
        let fy = ((y - pic.1) / pic.3).clamp(0.0, 1.0) as f64;
        (fx * self.size.0 as f64, fy * self.size.1 as f64)
    }

    fn next_number(&self) -> u32 {
        self.marks.iter().filter(|m| m.kind == Kind::Number).count() as u32 + 1
    }

    fn all(&self) -> Vec<&Mark> {
        self.marks.iter().chain(self.drawing.iter()).collect()
    }
}

impl Form for Annotate {
    fn title(&self) -> String {
        "Annotate Photo".into()
    }
    fn width(&self) -> f32 {
        640.0
    }
    fn canvas_height(&self) -> f32 {
        400.0
    }
    fn paint_canvas(&self, p: &Painter, r: R, pal: &Palette) {
        p.fill_round_rect(r.0, r.1, r.2, r.3, 12.0, Rgba(0.0, 0.0, 0.0, if pal.glass { 0.3 } else { 0.85 }));
        let pic = self.picture(r);
        p.image(&self.photo.rgba, self.photo.w, self.photo.h, pic);
        let marks = self.all();
        if marks.is_empty() {
            return;
        }
        // The overlay at twice the preview size, so it stays sharp on high-DPI screens.
        let (w, h) = (((pic.2 * 2.0) as u32).max(1), ((pic.3 * 2.0) as u32).max(1));
        let svg = svg(&marks, self.size.0, self.size.1);
        let mut cache = self.cache.borrow_mut();
        if !cache.as_ref().is_some_and(|(s, z, _)| *s == svg && *z == (w, h)) {
            if let Ok(img) = satsuma_engine::imaging::render_svg(&svg, w, h) {
                *cache = Some((svg, (w, h), Image { w: img.width(), h: img.height(), rgba: img.into_raw() }));
            }
        }
        if let Some((_, _, img)) = cache.as_ref() {
            p.image(&img.rgba, img.w, img.h, pic);
        }
    }
    fn canvas_down(&mut self, x: f32, y: f32, r: R) -> bool {
        let pic = self.picture(r);
        if !contains(pic, x, y) {
            return false;
        }
        let at = self.to_photo(pic, x, y);
        let kind = Kind::from(self.kind);
        let text = match kind {
            Kind::Number => self.next_number().to_string(),
            Kind::Text => self.text.trim().to_string(),
            _ => String::new(),
        };
        if kind == Kind::Text && text.is_empty() {
            return false;
        }
        self.drawing = Some(Mark { kind, a: at, b: at, colour: COLOURS[self.colour], size: self.weight, text });
        true
    }
    fn canvas_move(&mut self, x: f32, y: f32, r: R, held: bool) -> bool {
        if !held || self.drawing.is_none() {
            return false;
        }
        let at = self.to_photo(self.picture(r), x, y);
        if let Some(m) = self.drawing.as_mut() {
            if m.kind.placed() {
                // Text and numbers follow the pointer until released.
                m.a = at;
            }
            m.b = at;
        }
        true
    }
    fn canvas_up(&mut self, _x: f32, _y: f32, _r: R) -> bool {
        if let Some(m) = self.drawing.take() {
            let span = ((m.b.0 - m.a.0).powi(2) + (m.b.1 - m.a.1).powi(2)).sqrt();
            if m.kind.placed() || span > self.size.0.max(self.size.1) as f64 / 150.0 {
                self.marks.push(m);
            }
        }
        true
    }
    fn key(&mut self, vk: VIRTUAL_KEY) -> bool {
        match vk {
            VK_DELETE | VK_BACK => {
                self.marks.pop();
            }
            VK_Z if unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 => {
                self.marks.pop();
            }
            _ => return false,
        }
        true
    }
    fn fields(&self) -> Vec<Field> {
        let mut v = vec![
            Field::Choice { id: 1, label: "Draw".into(), choices: KINDS.iter().map(|s| s.to_string()).collect(), value: self.kind },
            Field::Colours { id: 2, label: "Colour".into(), colours: COLOURS.to_vec(), value: self.colour },
            Field::Choice { id: 3, label: "Size".into(), choices: SIZES.iter().map(|s| s.to_string()).collect(), value: self.weight },
        ];
        if Kind::from(self.kind) == Kind::Text {
            v.push(Field::Text { id: 4, label: "Text".into(), value: self.text.clone(), placeholder: "What to write".into() });
        }
        v.push(Field::Choice { id: 5, label: "".into(), choices: vec!["Undo".into(), "Clear All".into()], value: usize::MAX });
        v.push(Field::Note(match Kind::from(self.kind) {
            Kind::Text => "Type the text, then click the photo to place it (drag to move it while placing).".into(),
            Kind::Number => format!("Click the photo to place marker {}.", self.next_number()),
            _ => "Drag on the photo to draw. Delete or Ctrl+Z removes the last mark.".into(),
        }));
        v
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.kind = i.min(KINDS.len() - 1),
            (2, Change::Colour(i)) => self.colour = i.min(COLOURS.len() - 1),
            (3, Change::Choice(i)) => self.weight = i.min(2),
            (4, Change::Text(t)) => self.text = t,
            (5, Change::Choice(0)) => {
                self.marks.pop();
            }
            (5, Change::Choice(_)) => self.marks.clear(),
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Save".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        if self.marks.is_empty() {
            return Err("Draw something on the photo first.".into());
        }
        let marks: Vec<&Mark> = self.marks.iter().collect();
        Ok(Operation::Configured(Tool::AnnotateImage, ToolOptions::Annotate { svg: svg(&marks, self.size.0, self.size.1) }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_become_svg() {
        let m = Mark { kind: Kind::Text, a: (10.0, 20.0), b: (10.0, 20.0), colour: [255, 0, 0], size: 1, text: "a < b & \"c\"".into() };
        let s = svg(&[&m], 100, 50);
        assert!(s.contains("viewBox=\"0 0 100 50\""));
        assert!(s.contains("a &lt; b &amp; &quot;c&quot;"));
        assert!(s.contains("#ff0000"));
    }
}
