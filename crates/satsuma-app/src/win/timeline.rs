//! Trim Video and Trim Audio: start and end handles on a filmstrip or a waveform, a large
//! preview of the frame under the handle being moved, and exact times to type.

use super::form::{contains, Change, Field, Form, R};
use super::forms::clock;
use super::gfx::{Align, Painter, Rgba, Weight};
use super::theme::{Palette, ACCENT};
use satsuma_core::Tool;
use satsuma_engine::plan::ToolOptions;
use satsuma_engine::Operation;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

fn env() -> &'static satsuma_engine::Env {
    satsuma_engine::win::env()
}

/// "1:05.5", "65.5", "1:02:03" → seconds.
pub fn parse_clock(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut total = 0.0;
    for p in &parts {
        let v: f64 = p.trim().parse().ok()?;
        if !v.is_finite() || v < 0.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    Some(total)
}

pub struct Image {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

pub fn load_png(path: &std::path::Path) -> Option<Image> {
    let img = image::open(path).ok()?.to_rgba8();
    Some(Image { w: img.width(), h: img.height(), rgba: img.into_raw() })
}

/// A PNG cropped to a centred square (for filmstrip slots).
fn load_square(path: &std::path::Path) -> Option<Image> {
    let img = image::open(path).ok()?.to_rgba8();
    let side = img.width().min(img.height());
    let (x, y) = ((img.width() - side) / 2, (img.height() - side) / 2);
    let sq = image::imageops::crop_imm(&img, x, y, side, side).to_image();
    Some(Image { w: side, h: side, rgba: sq.into_raw() })
}

/// Fits `w`×`h` inside `r`, centred.
pub fn fit(w: u32, h: u32, r: R) -> R {
    let s = (r.2 / w as f32).min(r.3 / h as f32);
    let (fw, fh) = (w as f32 * s, h as f32 * s);
    (r.0 + (r.2 - fw) / 2.0, r.1 + (r.3 - fh) / 2.0, fw, fh)
}

/// What the background loads: thumbnails or peaks, and preview frames.
#[derive(Default)]
pub struct Loaded {
    pub thumbs: Vec<Image>,
    pub peaks: Vec<f32>,
    pub preview: Option<(f64, Image)>,
    pub strip_done: bool,
    pub preview_busy: bool,
}

/// Loads the strip (or waveform) once and preview frames on request, off the UI thread.
pub struct Media {
    pub path: PathBuf,
    pub video: bool,
    pub duration: f64,
    pub loaded: Arc<Mutex<Loaded>>,
    pub want: Option<f64>,
}

impl Media {
    pub fn open(path: PathBuf, video: bool) -> Option<Media> {
        let duration = satsuma_engine::probe::probe(env(), &path).ok()?.duration.filter(|d| *d > 0.05)?;
        let loaded = Arc::new(Mutex::new(Loaded::default()));
        let (l, p) = (loaded.clone(), path.clone());
        std::thread::spawn(move || {
            let dir = tempfile::tempdir().ok();
            if video {
                let thumbs = dir
                    .as_ref()
                    .and_then(|d| satsuma_engine::media::thumbnails(env(), &p, 10, 160, d.path()).ok())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|f| load_square(f))
                    .collect();
                if let Ok(mut l) = l.lock() {
                    l.thumbs = thumbs;
                }
            } else if let Ok(peaks) = satsuma_engine::media::peaks(env(), &p, 600) {
                if let Ok(mut l) = l.lock() {
                    l.peaks = peaks;
                }
            }
            if let Ok(mut l) = l.lock() {
                l.strip_done = true;
            }
        });
        let mut m = Media { path, video, duration, loaded, want: None };
        if video {
            m.want = Some(0.0);
        }
        Some(m)
    }

    /// Starts loading the wanted preview frame when none is loading.
    pub fn tick(&mut self) {
        let Some(at) = self.want else { return };
        let Ok(mut l) = self.loaded.lock() else { return };
        if l.preview_busy || l.preview.as_ref().is_some_and(|(t, _)| (t - at).abs() < 0.01) {
            if l.preview.as_ref().is_some_and(|(t, _)| (t - at).abs() < 0.01) {
                self.want = None;
            }
            return;
        }
        l.preview_busy = true;
        drop(l);
        let (loaded, path) = (self.loaded.clone(), self.path.clone());
        std::thread::spawn(move || {
            let frame = tempfile::tempdir().ok().and_then(|d| {
                let out = d.path().join("frame.png");
                satsuma_engine::media::frame_scaled(env(), &path, at, 640, &out).ok()?;
                load_png(&out)
            });
            if let Ok(mut l) = loaded.lock() {
                l.preview_busy = false;
                if let Some(f) = frame {
                    l.preview = Some((at, f));
                }
            }
        });
    }

    pub fn busy(&self) -> bool {
        self.want.is_some() || self.loaded.lock().map(|l| !l.strip_done || l.preview_busy).unwrap_or(false)
    }

    /// The large preview of the current frame.
    pub fn paint_preview(&self, p: &Painter, r: R, pal: &Palette) {
        p.fill_round_rect(r.0, r.1, r.2, r.3, 12.0, Rgba(0.0, 0.0, 0.0, if pal.glass { 0.35 } else { 0.85 }));
        let l = self.loaded.lock();
        match l.as_ref().ok().and_then(|l| l.preview.as_ref()) {
            Some((_, img)) => p.image(&img.rgba, img.w, img.h, fit(img.w, img.h, r)),
            None => p.text("Loading…", r, 12.0, Weight::Regular, Rgba(1.0, 1.0, 1.0, 0.6), Align::Center, false),
        }
    }

    /// The filmstrip or the waveform across `r`.
    pub fn paint_strip(&self, p: &Painter, r: R, pal: &Palette) {
        p.fill_round_rect(r.0, r.1, r.2, r.3, 8.0, if pal.glass { Rgba(0.0, 0.0, 0.0, 0.3) } else { pal.control });
        let Ok(l) = self.loaded.lock() else { return };
        if self.video {
            let n = l.thumbs.len();
            if n > 0 {
                let w = r.2 / n as f32;
                for (i, t) in l.thumbs.iter().enumerate() {
                    // Square thumbnails, stretched a little to fill their slot exactly.
                    p.image(&t.rgba, t.w, t.h, (r.0 + i as f32 * w, r.1, w + 0.5, r.3));
                }
            }
        } else if !l.peaks.is_empty() {
            let n = l.peaks.len();
            let mid = r.1 + r.3 / 2.0;
            let step = r.2 / n as f32;
            for (i, v) in l.peaks.iter().enumerate() {
                let x = r.0 + i as f32 * step;
                let h = (v * (r.3 / 2.0 - 4.0)).max(0.6);
                p.line(x, mid - h, x, mid + h, pal.secondary.alpha(0.8), step.max(1.0) * 0.8);
            }
        }
        if !l.strip_done {
            p.text("Loading…", r, 11.0, Weight::Regular, pal.secondary, Align::Center, false);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Handle {
    Start,
    End,
}

pub struct Trim {
    files: Vec<PathBuf>,
    media: Media,
    start: f64,
    end: f64,
    start_text: String,
    end_text: String,
    drag: Option<Handle>,
    active: Handle,
    /// Audio: trim the silence at the ends instead of keeping a chosen part.
    silence: bool,
}

const STRIP_H: f32 = 52.0;

impl Trim {
    pub fn new(files: Vec<PathBuf>, video: bool) -> Option<Trim> {
        let media = Media::open(files.first()?.clone(), video)?;
        let end = media.duration;
        Some(Trim { files, start: 0.0, end, start_text: clock(0.0), end_text: clock(end), media, drag: None, active: Handle::Start, silence: false })
    }

    fn strip(&self, r: R) -> R {
        if self.media.video {
            (r.0 + 12.0, r.1 + r.3 - STRIP_H - 22.0, r.2 - 24.0, STRIP_H)
        } else {
            (r.0 + 12.0, r.1 + 8.0, r.2 - 24.0, r.3 - 38.0)
        }
    }

    fn x_of(&self, s: R, t: f64) -> f32 {
        s.0 + s.2 * (t / self.media.duration) as f32
    }

    fn t_of(&self, s: R, x: f32) -> f64 {
        (((x - s.0) / s.2).clamp(0.0, 1.0) as f64) * self.media.duration
    }

    fn set(&mut self, h: Handle, t: f64) {
        let d = self.media.duration;
        match h {
            Handle::Start => self.start = t.clamp(0.0, (self.end - 0.1).max(0.0)),
            Handle::End => self.end = t.clamp((self.start + 0.1).min(d), d),
        }
        self.start_text = clock(self.start);
        self.end_text = clock(self.end);
        if self.media.video {
            self.media.want = Some(match h {
                Handle::Start => self.start,
                Handle::End => (self.end - 0.04).max(self.start),
            });
        }
    }
}

impl Form for Trim {
    fn title(&self) -> String {
        if self.media.video { "Trim Video".into() } else { "Trim Audio".into() }
    }
    fn width(&self) -> f32 {
        if self.media.video { 560.0 } else { 520.0 }
    }
    fn canvas_height(&self) -> f32 {
        if self.media.video { 380.0 } else { 150.0 }
    }
    fn paint_canvas(&self, p: &Painter, r: R, pal: &Palette) {
        if self.media.video {
            let preview = (r.0, r.1, r.2, r.3 - STRIP_H - 34.0);
            self.media.paint_preview(p, preview, pal);
        }
        let s = self.strip(r);
        self.media.paint_strip(p, s, pal);
        let dim = !self.silence;
        let (xs, xe) = (self.x_of(s, self.start), self.x_of(s, self.end));
        if dim {
            // Shade what goes; frame what stays.
            p.fill_round_rect(s.0, s.1, (xs - s.0).max(0.0), s.3, 0.0, Rgba(0.0, 0.0, 0.0, 0.6));
            p.fill_round_rect(xe, s.1, (s.0 + s.2 - xe).max(0.0), s.3, 0.0, Rgba(0.0, 0.0, 0.0, 0.6));
            p.stroke_round_rect(xs, s.1 - 1.0, (xe - xs).max(1.0), s.3 + 2.0, 6.0, ACCENT, 2.5);
            for (x, h) in [(xs, Handle::Start), (xe, Handle::End)] {
                let lit = self.active == h;
                p.fill_round_rect(x - 5.0, s.1 - 6.0, 10.0, s.3 + 12.0, 5.0, if lit { ACCENT } else { Rgba(1.0, 0.62, 0.32, 1.0) });
                p.line(x, s.1 + s.3 / 2.0 - 7.0, x, s.1 + s.3 / 2.0 + 7.0, Rgba::WHITE, 1.6);
            }
        }
        // Times under the strip.
        let ty = s.1 + s.3 + 6.0;
        p.text("0:00", (s.0, ty, 60.0, 16.0), 10.5, Weight::Regular, pal.tertiary, Align::Leading, false);
        let total = clock(self.media.duration);
        p.text(&total, (s.0 + s.2 - 60.0, ty, 60.0, 16.0), 10.5, Weight::Regular, pal.tertiary, Align::Leading, false);
        if dim {
            p.text(&format!("Keeps {}", clock(self.end - self.start)), (s.0, ty, s.2, 16.0), 11.0, Weight::Semibold, ACCENT, Align::Center, false);
        }
    }
    fn canvas_down(&mut self, x: f32, y: f32, r: R) -> bool {
        if self.silence {
            return false;
        }
        let s = self.strip(r);
        if !contains((s.0 - 12.0, s.1 - 10.0, s.2 + 24.0, s.3 + 20.0), x, y) {
            return false;
        }
        let (xs, xe) = (self.x_of(s, self.start), self.x_of(s, self.end));
        let h = if (x - xs).abs() <= (x - xe).abs() { Handle::Start } else { Handle::End };
        self.drag = Some(h);
        self.active = h;
        let t = self.t_of(s, x);
        self.set(h, t);
        true
    }
    fn canvas_move(&mut self, x: f32, _y: f32, r: R, held: bool) -> bool {
        match (held, self.drag) {
            (true, Some(h)) => {
                let t = self.t_of(self.strip(r), x);
                self.set(h, t);
                true
            }
            _ => false,
        }
    }
    fn canvas_up(&mut self, _x: f32, _y: f32, _r: R) -> bool {
        self.drag = None;
        true
    }
    fn key(&mut self, vk: VIRTUAL_KEY) -> bool {
        let step = if unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0 { 1.0 } else { 0.1 };
        let h = self.active;
        let now = if h == Handle::Start { self.start } else { self.end };
        match vk {
            VK_LEFT => self.set(h, now - step),
            VK_RIGHT => self.set(h, now + step),
            VK_HOME => self.active = Handle::Start,
            VK_END => self.active = Handle::End,
            _ => return false,
        }
        true
    }
    fn fields(&self) -> Vec<Field> {
        let mut v = Vec::new();
        if !self.media.video {
            v.push(Field::Choice { id: 3, label: "Trim".into(), choices: vec!["Keep a part".into(), "Silence at the ends".into()], value: self.silence as usize });
        }
        if self.silence {
            v.push(Field::Note("Removes the quiet at the start and the end; pauses inside stay.".into()));
        } else {
            v.push(Field::Text { id: 1, label: "Start".into(), value: self.start_text.clone(), placeholder: "0:00".into() });
            v.push(Field::Text { id: 2, label: "End".into(), value: self.end_text.clone(), placeholder: clock(self.media.duration) });
            v.push(Field::Note("Drag the handles, or type exact times. ← → move the handle by a tenth of a second (Shift: a second).".into()));
        }
        v
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (3, Change::Choice(i)) => self.silence = i == 1,
            (1, Change::Text(t)) => {
                if let Some(v) = parse_clock(&t) {
                    self.active = Handle::Start;
                    self.set(Handle::Start, v);
                }
                self.start_text = t;
            }
            (2, Change::Text(t)) => {
                if let Some(v) = parse_clock(&t) {
                    self.active = Handle::End;
                    self.set(Handle::End, v);
                }
                self.end_text = t;
            }
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Trim".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        if !self.media.video && self.silence {
            return Ok(Operation::Tool(Tool::TrimAudio));
        }
        if self.start <= 0.01 && self.end >= self.media.duration - 0.01 {
            return Err("Move a handle to choose the part to keep.".into());
        }
        let tool = if self.media.video { Tool::TrimVideo } else { Tool::TrimAudio };
        Ok(Operation::Configured(tool, ToolOptions::Trim { start: self.start, end: self.end }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_parse() {
        assert_eq!(parse_clock("1:05.5"), Some(65.5));
        assert_eq!(parse_clock("65.5"), Some(65.5));
        assert_eq!(parse_clock("1:02:03"), Some(3723.0));
        assert_eq!(parse_clock("x"), None);
        assert_eq!(parse_clock("nan"), None);
        assert_eq!(parse_clock("inf"), None);
        assert_eq!(parse_clock("1:2:3:4"), None);
    }
}
