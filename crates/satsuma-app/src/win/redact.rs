//! Redact Photo: boxes over what to hide — drawn by hand, or found as text (OCR) or faces —
//! then blurred, pixelated or filled. The preview hides the same way as the output.

use super::form::{contains, Change, Field, Form, R};
use super::gfx::{Painter, Rgba};
use super::theme::{Palette, ACCENT};
use super::timeline::{fit, Image};
use image::RgbaImage;
use satsuma_core::Tool;
use satsuma_engine::plan::{RedactStyle, ToolOptions};
use satsuma_engine::Operation;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

const STYLES: [RedactStyle; 3] = [RedactStyle::Blur, RedactStyle::Pixelate, RedactStyle::Solid];

#[derive(Default)]
struct Finder {
    busy: Option<&'static str>,
    found: Vec<[f64; 4]>,
    message: Option<String>,
}

#[derive(Clone, Copy)]
enum Grab {
    /// Drawing a new box from this corner (fractions).
    New(f64, f64),
    /// Moving the selected box; the pointer's offset from its corner.
    Move(f64, f64),
}

pub struct Redact {
    files: Vec<PathBuf>,
    small: RgbaImage,
    size: (u32, u32),
    boxes: Vec<[f64; 4]>,
    selected: Option<usize>,
    style: usize,
    grab: Option<Grab>,
    finder: Arc<Mutex<Finder>>,
    message: String,
    cache: RefCell<Option<(String, Image)>>,
}

/// Grows a box by `k` of its height on every side.
fn pad(b: [f64; 4], k: f64) -> [f64; 4] {
    let m = b[3] * k;
    [b[0] - m, b[1] - m, b[2] + 2.0 * m, b[3] + 2.0 * m]
}

impl Redact {
    pub fn new(files: Vec<PathBuf>) -> Option<Redact> {
        let full = satsuma_engine::imaging::load(files.first()?).ok()?;
        let size = full.dimensions();
        let k = (1400.0 / size.0.max(size.1) as f64).min(1.0);
        let small = if k < 1.0 {
            image::imageops::resize(&*full, ((size.0 as f64 * k) as u32).max(1), ((size.1 as f64 * k) as u32).max(1), image::imageops::FilterType::Triangle)
        } else {
            (*full).clone()
        };
        Some(Redact {
            files,
            small,
            size,
            boxes: Vec::new(),
            selected: None,
            style: 0,
            grab: None,
            finder: Arc::new(Mutex::new(Finder::default())),
            message: String::new(),
            cache: RefCell::new(None),
        })
    }

    fn picture(&self, r: R) -> R {
        fit(self.size.0, self.size.1, (r.0 + 8.0, r.1 + 8.0, r.2 - 16.0, r.3 - 16.0))
    }

    fn frac(&self, pic: R, x: f32, y: f32) -> (f64, f64) {
        (((x - pic.0) / pic.2).clamp(0.0, 1.0) as f64, ((y - pic.1) / pic.3).clamp(0.0, 1.0) as f64)
    }

    fn screen(pic: R, b: &[f64; 4]) -> R {
        (pic.0 + pic.2 * b[0] as f32, pic.1 + pic.3 * b[1] as f32, pic.2 * b[2] as f32, pic.3 * b[3] as f32)
    }

    fn find(&mut self, faces: bool) {
        let Ok(mut f) = self.finder.lock() else { return };
        if f.busy.is_some() {
            return;
        }
        f.busy = Some(if faces { "Looking for faces…" } else { "Looking for text…" });
        drop(f);
        let (finder, path) = (self.finder.clone(), self.files[0].clone());
        std::thread::spawn(move || {
            let found = if faces { satsuma_engine::ocr::face_boxes(&path) } else { satsuma_engine::ocr::word_boxes(&path) };
            let Ok(mut f) = finder.lock() else { return };
            f.busy = None;
            match found {
                Ok(boxes) if boxes.is_empty() => f.message = Some(if faces { "No faces found.".into() } else { "No text found.".into() }),
                Ok(boxes) => {
                    let n = boxes.len();
                    let what = match (faces, n) {
                        (true, 1) => "face".to_string(),
                        (true, _) => "faces".to_string(),
                        (false, 1) => "word".to_string(),
                        (false, _) => "words".to_string(),
                    };
                    f.message = Some(format!("Found {n} {what}."));
                    f.found = boxes.into_iter().map(|b| pad(b, if faces { 0.12 } else { 0.18 })).collect();
                }
                Err(e) => f.message = Some(e.to_string()),
            }
        });
    }
}

impl Form for Redact {
    fn title(&self) -> String {
        "Redact Photo".into()
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
        // While a box is being drawn or moved, only its outline shows; hiding follows on release.
        let settled = self.grab.is_none();
        let key = format!("{:?}{}", self.boxes, self.style);
        let mut cache = self.cache.borrow_mut();
        if settled && !self.boxes.is_empty() && !cache.as_ref().is_some_and(|(k, _)| *k == key) {
            let hidden = satsuma_engine::imaging::redact(&self.small, &self.boxes, STYLES[self.style]);
            *cache = Some((key.clone(), Image { w: hidden.width(), h: hidden.height(), rgba: hidden.as_raw().clone() }));
        }
        match cache.as_ref().filter(|(k, _)| *k == key || !settled).filter(|_| !self.boxes.is_empty()) {
            Some((_, img)) => p.image(&img.rgba, img.w, img.h, pic),
            None => p.image(self.small.as_raw(), self.small.width(), self.small.height(), pic),
        }
        for (i, b) in self.boxes.iter().enumerate() {
            let s = Self::screen(pic, b);
            let lit = self.selected == Some(i);
            p.stroke_round_rect(s.0, s.1, s.2, s.3, 3.0, if lit { ACCENT } else { Rgba(1.0, 1.0, 1.0, 0.55) }, if lit { 2.0 } else { 1.0 });
        }
    }
    fn canvas_down(&mut self, x: f32, y: f32, r: R) -> bool {
        let pic = self.picture(r);
        if !contains(pic, x, y) {
            return false;
        }
        let (fx, fy) = self.frac(pic, x, y);
        // The last-drawn box on top wins.
        if let Some(i) = self.boxes.iter().rposition(|b| contains(Self::screen(pic, b), x, y)) {
            self.selected = Some(i);
            self.grab = Some(Grab::Move(fx - self.boxes[i][0], fy - self.boxes[i][1]));
            return true;
        }
        self.boxes.push([fx, fy, 0.0, 0.0]);
        self.selected = Some(self.boxes.len() - 1);
        self.grab = Some(Grab::New(fx, fy));
        true
    }
    fn canvas_move(&mut self, x: f32, y: f32, r: R, held: bool) -> bool {
        let (Some(grab), Some(i), true) = (self.grab, self.selected, held) else { return false };
        let (fx, fy) = self.frac(self.picture(r), x, y);
        let Some(b) = self.boxes.get_mut(i) else { return false };
        match grab {
            Grab::New(sx, sy) => *b = [sx.min(fx), sy.min(fy), (fx - sx).abs(), (fy - sy).abs()],
            Grab::Move(ox, oy) => {
                b[0] = (fx - ox).clamp(0.0, 1.0 - b[2]);
                b[1] = (fy - oy).clamp(0.0, 1.0 - b[3]);
            }
        }
        true
    }
    fn canvas_up(&mut self, _x: f32, _y: f32, _r: R) -> bool {
        if let (Some(Grab::New(..)), Some(i)) = (self.grab, self.selected) {
            if self.boxes.get(i).is_some_and(|b| b[2] < 0.01 || b[3] < 0.01) {
                self.boxes.remove(i);
                self.selected = None;
            }
        }
        self.grab = None;
        true
    }
    fn key(&mut self, vk: VIRTUAL_KEY) -> bool {
        match vk {
            VK_DELETE | VK_BACK => {
                if let Some(i) = self.selected.filter(|i| *i < self.boxes.len()) {
                    self.boxes.remove(i);
                } else {
                    self.boxes.pop();
                }
                self.selected = None;
            }
            _ => return false,
        }
        true
    }
    fn fields(&self) -> Vec<Field> {
        let busy = self.finder.lock().ok().and_then(|f| f.busy);
        let note = match busy {
            Some(b) => b.to_string(),
            None if !self.message.is_empty() => format!("{} Drag to add more; click a box and press Delete to remove it.", self.message),
            None => "Drag over what to hide, or let Satsuma find text or faces. Click a box and press Delete to remove it.".into(),
        };
        vec![
            Field::Choice { id: 1, label: "Hide with".into(), choices: vec!["Blur".into(), "Pixelate".into(), "Black Box".into()], value: self.style },
            Field::Choice { id: 2, label: "Find".into(), choices: vec!["Text".into(), "Faces".into()], value: usize::MAX },
            Field::Choice { id: 3, label: "".into(), choices: vec!["Remove Selected".into(), "Clear All".into()], value: usize::MAX },
            Field::Note(note),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.style = i.min(STYLES.len() - 1),
            (2, Change::Choice(i)) => self.find(i == 1),
            (3, Change::Choice(0)) => {
                if let Some(i) = self.selected.filter(|i| *i < self.boxes.len()) {
                    self.boxes.remove(i);
                }
                self.selected = None;
            }
            (3, Change::Choice(_)) => {
                self.boxes.clear();
                self.selected = None;
            }
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Redact".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        if self.boxes.is_empty() {
            return Err("Mark at least one area to hide.".into());
        }
        Ok(Operation::Configured(Tool::RedactImage, ToolOptions::Redact { boxes: self.boxes.clone(), style: STYLES[self.style] }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
    fn animating(&self) -> bool {
        self.finder.lock().map(|f| f.busy.is_some() || f.message.is_some()).unwrap_or(false)
    }
    fn tick(&mut self) {
        let Ok(mut f) = self.finder.lock() else { return };
        if f.busy.is_some() {
            return;
        }
        if let Some(m) = f.message.take() {
            self.message = m;
            let found = std::mem::take(&mut f.found);
            self.boxes.extend(found);
        }
    }
}
