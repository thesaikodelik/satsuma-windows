//! Redact Video: boxes over a frame of the video, hidden by blur, pixels or black, through
//! the whole clip or a part of it chosen on the filmstrip.

use super::form::{contains, Change, Field, Form, R};
use super::forms::clock;
use super::gfx::{Align, Painter, Rgba, Weight};
use super::theme::{Palette, ACCENT};
use super::timeline::{fit, parse_clock, Image, Media};
use satsuma_core::Tool;
use satsuma_engine::plan::{RedactStyle, ToolOptions};
use satsuma_engine::Operation;
use std::cell::RefCell;
use std::path::PathBuf;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

const STYLES: [RedactStyle; 3] = [RedactStyle::Blur, RedactStyle::Pixelate, RedactStyle::Solid];
const STRIP_H: f32 = 44.0;

#[derive(Clone, Copy, PartialEq)]
enum Grab {
    New(f64, f64),
    Move(f64, f64),
    From,
    To,
    Scrub,
}

pub struct RedactVideo {
    files: Vec<PathBuf>,
    media: Media,
    size: (u32, u32),
    boxes: Vec<[f64; 4]>,
    selected: Option<usize>,
    style: usize,
    part: bool,
    from: f64,
    to: f64,
    from_text: String,
    to_text: String,
    /// The time of the frame shown.
    at: f64,
    grab: Option<Grab>,
    cache: RefCell<Option<(String, Image)>>,
}

impl RedactVideo {
    pub fn new(files: Vec<PathBuf>) -> Option<RedactVideo> {
        let path = files.first()?.clone();
        let size = satsuma_engine::probe::probe(satsuma_engine::win::env(), &path).ok()?.video?;
        let mut media = Media::open(path, true)?;
        let at = media.duration / 3.0;
        media.want = Some(at);
        let to = media.duration;
        Some(RedactVideo {
            files,
            media,
            size,
            boxes: Vec::new(),
            selected: None,
            style: 0,
            part: false,
            from: 0.0,
            to,
            from_text: clock(0.0),
            to_text: clock(to),
            at,
            grab: None,
            cache: RefCell::new(None),
        })
    }

    fn preview(&self, r: R) -> R {
        (r.0, r.1, r.2, r.3 - STRIP_H - 30.0)
    }

    fn picture(&self, r: R) -> R {
        fit(self.size.0, self.size.1, self.preview(r))
    }

    fn strip(&self, r: R) -> R {
        (r.0 + 12.0, r.1 + r.3 - STRIP_H - 20.0, r.2 - 24.0, STRIP_H)
    }

    fn x_of(&self, s: R, t: f64) -> f32 {
        s.0 + s.2 * (t / self.media.duration) as f32
    }

    fn t_of(&self, s: R, x: f32) -> f64 {
        (((x - s.0) / s.2).clamp(0.0, 1.0) as f64) * self.media.duration
    }

    fn frac(pic: R, x: f32, y: f32) -> (f64, f64) {
        (((x - pic.0) / pic.2).clamp(0.0, 1.0) as f64, ((y - pic.1) / pic.3).clamp(0.0, 1.0) as f64)
    }

    fn screen(pic: R, b: &[f64; 4]) -> R {
        (pic.0 + pic.2 * b[0] as f32, pic.1 + pic.3 * b[1] as f32, pic.2 * b[2] as f32, pic.3 * b[3] as f32)
    }

    fn show(&mut self, t: f64) {
        self.at = t.clamp(0.0, (self.media.duration - 0.04).max(0.0));
        self.media.want = Some(self.at);
    }

    fn set_from(&mut self, t: f64) {
        self.from = t.clamp(0.0, (self.to - 0.1).max(0.0));
        self.from_text = clock(self.from);
        self.show(self.from);
    }

    fn set_to(&mut self, t: f64) {
        self.to = t.clamp((self.from + 0.1).min(self.media.duration), self.media.duration);
        self.to_text = clock(self.to);
        self.show(self.to - 0.04);
    }

    /// Whether the frame shown falls where the boxes apply.
    fn hiding_now(&self) -> bool {
        !self.part || (self.at >= self.from - 0.02 && self.at <= self.to + 0.02)
    }
}

impl Form for RedactVideo {
    fn title(&self) -> String {
        "Redact Video".into()
    }
    fn width(&self) -> f32 {
        600.0
    }
    fn canvas_height(&self) -> f32 {
        420.0
    }
    fn paint_canvas(&self, p: &Painter, r: R, pal: &Palette) {
        let area = self.preview(r);
        let pic = self.picture(r);
        self.media.paint_preview(p, area, pal);
        // The frame with the boxes hidden, as the output will look (once settled).
        if self.grab.is_none() && !self.boxes.is_empty() && self.hiding_now() {
            if let Ok(l) = self.media.loaded.lock() {
                if let Some((t, img)) = l.preview.as_ref() {
                    let key = format!("{t}{:?}{}", self.boxes, self.style);
                    let mut cache = self.cache.borrow_mut();
                    if !cache.as_ref().is_some_and(|(k, _)| *k == key) {
                        if let Some(frame) = image::RgbaImage::from_raw(img.w, img.h, img.rgba.clone()) {
                            let hidden = satsuma_engine::imaging::redact(&frame, &self.boxes, STYLES[self.style]);
                            *cache = Some((key.clone(), Image { w: hidden.width(), h: hidden.height(), rgba: hidden.as_raw().clone() }));
                        }
                    }
                    if let Some((_, img)) = cache.as_ref().filter(|(k, _)| *k == key) {
                        p.image(&img.rgba, img.w, img.h, fit(img.w, img.h, area));
                    }
                }
            }
        }
        for (i, b) in self.boxes.iter().enumerate() {
            let s = Self::screen(pic, b);
            let lit = self.selected == Some(i);
            p.stroke_round_rect(s.0, s.1, s.2, s.3, 3.0, if lit { ACCENT } else { Rgba(1.0, 1.0, 1.0, 0.6) }, if lit { 2.0 } else { 1.0 });
        }
        let s = self.strip(r);
        self.media.paint_strip(p, s, pal);
        if self.part {
            let (xa, xb) = (self.x_of(s, self.from), self.x_of(s, self.to));
            p.fill_round_rect(s.0, s.1, (xa - s.0).max(0.0), s.3, 0.0, Rgba(0.0, 0.0, 0.0, 0.6));
            p.fill_round_rect(xb, s.1, (s.0 + s.2 - xb).max(0.0), s.3, 0.0, Rgba(0.0, 0.0, 0.0, 0.6));
            p.stroke_round_rect(xa, s.1 - 1.0, (xb - xa).max(1.0), s.3 + 2.0, 6.0, ACCENT, 2.5);
            for x in [xa, xb] {
                p.fill_round_rect(x - 5.0, s.1 - 6.0, 10.0, s.3 + 12.0, 5.0, ACCENT);
                p.line(x, s.1 + s.3 / 2.0 - 7.0, x, s.1 + s.3 / 2.0 + 7.0, Rgba::WHITE, 1.6);
            }
        }
        // The playhead: the frame shown above.
        let xp = self.x_of(s, self.at);
        p.fill_round_rect(xp - 1.5, s.1 - 3.0, 3.0, s.3 + 6.0, 1.5, Rgba::WHITE);
        let ty = s.1 + s.3 + 4.0;
        p.text("0:00", (s.0, ty, 60.0, 16.0), 10.5, Weight::Regular, pal.tertiary, Align::Leading, false);
        p.text(&clock(self.media.duration), (s.0 + s.2 - 30.0, ty, 60.0, 16.0), 10.5, Weight::Regular, pal.tertiary, Align::Leading, false);
        p.text(&format!("Showing {}", clock(self.at)), (s.0, ty, s.2, 16.0), 11.0, Weight::Semibold, pal.secondary, Align::Center, false);
    }
    fn canvas_down(&mut self, x: f32, y: f32, r: R) -> bool {
        let pic = self.picture(r);
        let s = self.strip(r);
        if contains((s.0 - 12.0, s.1 - 10.0, s.2 + 24.0, s.3 + 20.0), x, y) {
            let near = |t: f64| (x - self.x_of(s, t)).abs() <= 8.0;
            let grab = if self.part && near(self.from) {
                Grab::From
            } else if self.part && near(self.to) {
                Grab::To
            } else {
                Grab::Scrub
            };
            self.grab = Some(grab);
            if grab == Grab::Scrub {
                self.show(self.t_of(s, x));
            }
            return true;
        }
        if !contains(pic, x, y) {
            return false;
        }
        let (fx, fy) = Self::frac(pic, x, y);
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
        let Some(grab) = self.grab.filter(|_| held) else { return false };
        let s = self.strip(r);
        match grab {
            Grab::Scrub => self.show(self.t_of(s, x)),
            Grab::From => self.set_from(self.t_of(s, x)),
            Grab::To => self.set_to(self.t_of(s, x)),
            Grab::New(sx, sy) | Grab::Move(sx, sy) => {
                let (fx, fy) = Self::frac(self.picture(r), x, y);
                let Some(b) = self.selected.and_then(|i| self.boxes.get_mut(i)) else { return false };
                if let Grab::New(..) = grab {
                    *b = [sx.min(fx), sy.min(fy), (fx - sx).abs(), (fy - sy).abs()];
                } else {
                    b[0] = (fx - sx).clamp(0.0, 1.0 - b[2]);
                    b[1] = (fy - sy).clamp(0.0, 1.0 - b[3]);
                }
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
            VK_LEFT => self.show(self.at - 1.0),
            VK_RIGHT => self.show(self.at + 1.0),
            _ => return false,
        }
        true
    }
    fn fields(&self) -> Vec<Field> {
        let mut v = vec![
            Field::Choice { id: 1, label: "Hide with".into(), choices: vec!["Blur".into(), "Pixelate".into(), "Black Box".into()], value: self.style },
            Field::Choice { id: 2, label: "When".into(), choices: vec!["Whole Clip".into(), "Part of It".into()], value: self.part as usize },
        ];
        if self.part {
            v.push(Field::Text { id: 3, label: "From".into(), value: self.from_text.clone(), placeholder: "0:00".into() });
            v.push(Field::Text { id: 4, label: "To".into(), value: self.to_text.clone(), placeholder: clock(self.media.duration) });
        }
        v.push(Field::Note(if self.part {
            "Drag over what to hide; drag the orange handles to choose when. Click the strip to see another moment.".into()
        } else {
            "Drag over what to hide. Click the strip to check other moments of the clip.".into()
        }));
        v
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.style = i.min(STYLES.len() - 1),
            (2, Change::Choice(i)) => self.part = i == 1,
            (3, Change::Text(t)) => {
                if let Some(v) = parse_clock(&t) {
                    self.set_from(v);
                }
                self.from_text = t;
            }
            (4, Change::Text(t)) => {
                if let Some(v) = parse_clock(&t) {
                    self.set_to(v);
                }
                self.to_text = t;
            }
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Redact".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        if self.boxes.is_empty() {
            return Err("Drag over what to hide first.".into());
        }
        let whole = !self.part || (self.from <= 0.01 && self.to >= self.media.duration - 0.01);
        let (from, to) = if whole { (0.0, None) } else { (self.from, Some(self.to)) };
        Ok(Operation::Configured(Tool::RedactVideo, ToolOptions::RedactVideo { boxes: self.boxes.clone(), style: STYLES[self.style], from, to }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
    fn animating(&self) -> bool {
        self.media.busy()
    }
    fn tick(&mut self) {
        self.media.tick();
    }
}
