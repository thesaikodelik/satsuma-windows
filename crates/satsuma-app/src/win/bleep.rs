//! Bleep Audio: mark the parts to hide on the waveform, then beep or mute them.

use super::form::{contains, Change, Field, Form, R};
use super::forms::clock;
use super::gfx::{Align, Painter, Rgba, Weight};
use super::theme::{Palette, ACCENT};
use super::timeline::Media;
use satsuma_core::Tool;
use satsuma_engine::plan::ToolOptions;
use satsuma_engine::Operation;
use std::path::PathBuf;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, PartialEq)]
enum Grab {
    /// A new range being drawn from where the pointer went down.
    New(f64),
    Start,
    End,
    /// Moving the whole range; the pointer's offset from its start.
    Move(f64),
}

pub struct Bleep {
    files: Vec<PathBuf>,
    media: Media,
    ranges: Vec<(f64, f64)>,
    selected: Option<usize>,
    grab: Option<Grab>,
    tone: bool,
}

const MIN: f64 = 0.05;

impl Bleep {
    pub fn new(files: Vec<PathBuf>) -> Option<Bleep> {
        let media = Media::open(files.first()?.clone(), false)?;
        Some(Bleep { files, media, ranges: Vec::new(), selected: None, grab: None, tone: true })
    }

    fn strip(&self, r: R) -> R {
        (r.0 + 12.0, r.1 + 10.0, r.2 - 24.0, r.3 - 40.0)
    }

    fn x_of(&self, s: R, t: f64) -> f32 {
        s.0 + s.2 * (t / self.media.duration) as f32
    }

    fn t_of(&self, s: R, x: f32) -> f64 {
        (((x - s.0) / s.2).clamp(0.0, 1.0) as f64) * self.media.duration
    }

    /// Sorts the ranges and joins overlapping ones, keeping the selection on the same range.
    fn tidy(&mut self) {
        let keep = self.selected.and_then(|i| self.ranges.get(i).copied());
        self.ranges.retain(|(a, b)| b - a >= MIN / 2.0);
        self.ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<(f64, f64)> = Vec::new();
        for (a, b) in self.ranges.drain(..) {
            match out.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => out.push((a, b)),
            }
        }
        self.ranges = out;
        self.selected = keep.and_then(|(a, _)| self.ranges.iter().position(|(s, e)| *s <= a + 1e-9 && a <= *e + 1e-9));
    }

    fn total(&self) -> f64 {
        self.ranges.iter().map(|(a, b)| b - a).sum()
    }
}

impl Form for Bleep {
    fn title(&self) -> String {
        "Bleep Audio".into()
    }
    fn width(&self) -> f32 {
        560.0
    }
    fn canvas_height(&self) -> f32 {
        170.0
    }
    fn paint_canvas(&self, p: &Painter, r: R, pal: &Palette) {
        let s = self.strip(r);
        self.media.paint_strip(p, s, pal);
        for (i, (a, b)) in self.ranges.iter().enumerate() {
            let (xa, xb) = (self.x_of(s, *a), self.x_of(s, *b));
            let lit = self.selected == Some(i);
            p.fill_round_rect(xa, s.1, (xb - xa).max(2.0), s.3, 4.0, ACCENT.alpha(if lit { 0.45 } else { 0.3 }));
            p.stroke_round_rect(xa, s.1, (xb - xa).max(2.0), s.3, 4.0, ACCENT.alpha(if lit { 1.0 } else { 0.7 }), if lit { 2.0 } else { 1.2 });
            if lit {
                for x in [xa, xb] {
                    p.fill_round_rect(x - 4.0, s.1 + s.3 / 2.0 - 12.0, 8.0, 24.0, 4.0, ACCENT);
                    p.line(x, s.1 + s.3 / 2.0 - 6.0, x, s.1 + s.3 / 2.0 + 6.0, Rgba::WHITE, 1.4);
                }
            }
        }
        let ty = s.1 + s.3 + 6.0;
        p.text("0:00", (s.0, ty, 60.0, 16.0), 10.5, Weight::Regular, pal.tertiary, Align::Leading, false);
        p.text(&clock(self.media.duration), (s.0 + s.2 - 30.0, ty, 60.0, 16.0), 10.5, Weight::Regular, pal.tertiary, Align::Leading, false);
        let middle = match self.selected.and_then(|i| self.ranges.get(i)) {
            Some((a, b)) => format!("{} – {}", clock(*a), clock(*b)),
            None if self.ranges.is_empty() => "Drag across the waveform to mark a part".into(),
            None => format!("{} marked · {}", self.ranges.len(), clock(self.total())),
        };
        p.text(&middle, (s.0, ty, s.2, 16.0), 11.0, Weight::Semibold, if self.ranges.is_empty() { pal.secondary } else { ACCENT }, Align::Center, false);
    }
    fn canvas_down(&mut self, x: f32, y: f32, r: R) -> bool {
        let s = self.strip(r);
        if !contains((s.0 - 8.0, s.1 - 6.0, s.2 + 16.0, s.3 + 12.0), x, y) {
            return false;
        }
        let t = self.t_of(s, x);
        // The selected range's edges first, then any range's body, then a new range.
        if let Some(i) = self.selected.filter(|i| *i < self.ranges.len()) {
            let (a, b) = self.ranges[i];
            if (x - self.x_of(s, a)).abs() <= 7.0 {
                self.grab = Some(Grab::Start);
                return true;
            }
            if (x - self.x_of(s, b)).abs() <= 7.0 {
                self.grab = Some(Grab::End);
                return true;
            }
        }
        if let Some(i) = self.ranges.iter().position(|(a, b)| t >= *a && t <= *b) {
            self.selected = Some(i);
            self.grab = Some(Grab::Move(t - self.ranges[i].0));
            return true;
        }
        self.ranges.push((t, t));
        self.selected = Some(self.ranges.len() - 1);
        self.grab = Some(Grab::New(t));
        true
    }
    fn canvas_move(&mut self, x: f32, _y: f32, r: R, held: bool) -> bool {
        let (Some(grab), Some(i), true) = (self.grab, self.selected, held) else { return false };
        let s = self.strip(r);
        let t = self.t_of(s, x);
        let d = self.media.duration;
        let Some(range) = self.ranges.get_mut(i) else { return false };
        match grab {
            Grab::New(from) => *range = (from.min(t), from.max(t)),
            Grab::Start => range.0 = t.min(range.1 - MIN).max(0.0),
            Grab::End => range.1 = t.max(range.0 + MIN).min(d),
            Grab::Move(offset) => {
                let len = range.1 - range.0;
                let a = (t - offset).clamp(0.0, (d - len).max(0.0));
                *range = (a, a + len);
            }
        }
        true
    }
    fn canvas_up(&mut self, _x: f32, _y: f32, _r: R) -> bool {
        if let (Some(Grab::New(_)), Some(i)) = (self.grab, self.selected) {
            // A click without a drag marks half a second around the point.
            if let Some(range) = self.ranges.get_mut(i).filter(|(a, b)| b - a < MIN) {
                let d = self.media.duration;
                let a = (range.0 - 0.25).max(0.0);
                *range = (a, (a + 0.5).min(d));
            }
        }
        self.grab = None;
        self.tidy();
        true
    }
    fn key(&mut self, vk: VIRTUAL_KEY) -> bool {
        match vk {
            VK_DELETE | VK_BACK => {
                if let Some(i) = self.selected.filter(|i| *i < self.ranges.len()) {
                    self.ranges.remove(i);
                    self.selected = None;
                }
            }
            _ => return false,
        }
        true
    }
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::Choice { id: 1, label: "Sound".into(), choices: vec!["Beep".into(), "Silence".into()], value: if self.tone { 0 } else { 1 } },
            Field::Choice { id: 2, label: "".into(), choices: vec!["Remove Marked Part".into(), "Clear All".into()], value: usize::MAX },
            Field::Note("Drag across the waveform to mark each part to hide; drag a part to move it, or its edges to resize it.".into()),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.tone = i == 0,
            (2, Change::Choice(0)) => {
                if let Some(i) = self.selected.filter(|i| *i < self.ranges.len()) {
                    self.ranges.remove(i);
                }
                self.selected = None;
            }
            (2, Change::Choice(_)) => {
                self.ranges.clear();
                self.selected = None;
            }
            _ => {}
        }
    }
    fn action(&self) -> String {
        if self.tone { "Bleep".into() } else { "Mute".into() }
    }
    fn operation(&self) -> Result<Operation, String> {
        if self.ranges.is_empty() {
            return Err("Mark at least one part on the waveform.".into());
        }
        Ok(Operation::Configured(Tool::RedactAudio, ToolOptions::Bleep { ranges: self.ranges.clone(), tone: self.tone }))
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
