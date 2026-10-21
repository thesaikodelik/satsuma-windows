//! Crop Video: a frame of the video with a crop box to drag, resize or fit to a ratio.

use super::form::{contains, Change, Field, Form, R};
use super::gfx::{Painter, Rgba};
use super::theme::{Palette, ACCENT};
use super::timeline::{fit, Media};
use satsuma_core::Tool;
use satsuma_engine::plan::ToolOptions;
use satsuma_engine::Operation;
use std::path::PathBuf;

const RATIOS: [(&str, f64); 6] = [("Free", 0.0), ("16:9", 16.0 / 9.0), ("9:16", 9.0 / 16.0), ("1:1", 1.0), ("4:3", 4.0 / 3.0), ("4:5", 0.8)];

#[derive(Clone, Copy, PartialEq)]
enum Grab {
    Move,
    /// Which edges move: left, top, right, bottom.
    Edges(bool, bool, bool, bool),
}

pub struct CropVideo {
    files: Vec<PathBuf>,
    media: Media,
    size: (u32, u32),
    /// The crop in fractions of the picture.
    rect: (f64, f64, f64, f64),
    ratio: usize,
    grab: Option<(Grab, (f32, f32), (f64, f64, f64, f64))>,
}

impl CropVideo {
    pub fn new(files: Vec<PathBuf>) -> Option<CropVideo> {
        let path = files.first()?.clone();
        let size = satsuma_engine::probe::probe(satsuma_engine::win::env(), &path).ok()?.video?;
        let mut media = Media::open(path, true)?;
        media.want = Some(media.duration / 3.0);
        Some(CropVideo { files, media, size, rect: (0.1, 0.1, 0.8, 0.8), ratio: 0, grab: None })
    }

    fn picture(&self, r: R) -> R {
        fit(self.size.0, self.size.1, (r.0, r.1, r.2, r.3))
    }

    fn screen(&self, pic: R) -> R {
        let (x, y, w, h) = self.rect;
        (pic.0 + pic.2 * x as f32, pic.1 + pic.3 * y as f32, pic.2 * w as f32, pic.3 * h as f32)
    }

    /// Applies the ratio by trimming the crop around its centre.
    fn apply_ratio(&mut self) {
        let ratio = RATIOS[self.ratio].1;
        if ratio <= 0.0 {
            return;
        }
        let (vw, vh) = (self.size.0 as f64, self.size.1 as f64);
        let (x, y, w, h) = self.rect;
        let (pw, ph) = (w * vw, h * vh);
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (nw, nh) = if pw / ph > ratio { (ph * ratio, ph) } else { (pw, pw / ratio) };
        let (fw, fh) = (nw / vw, nh / vh);
        self.rect = ((cx - fw / 2.0).clamp(0.0, 1.0 - fw), (cy - fh / 2.0).clamp(0.0, 1.0 - fh), fw, fh);
    }

    fn pixels(&self) -> (u32, u32) {
        let even = |v: f64| ((v.round() as u32) / 2 * 2).max(2);
        (even(self.rect.2 * self.size.0 as f64), even(self.rect.3 * self.size.1 as f64))
    }
}

impl Form for CropVideo {
    fn title(&self) -> String {
        "Crop Video".into()
    }
    fn width(&self) -> f32 {
        580.0
    }
    fn canvas_height(&self) -> f32 {
        330.0
    }
    fn paint_canvas(&self, p: &Painter, r: R, pal: &Palette) {
        let pic = self.picture(r);
        self.media.paint_preview(p, r, pal);
        let c = self.screen(pic);
        // Shade outside the crop.
        let shade = Rgba(0.0, 0.0, 0.0, 0.55);
        p.fill_round_rect(pic.0, pic.1, pic.2, c.1 - pic.1, 0.0, shade);
        p.fill_round_rect(pic.0, c.1 + c.3, pic.2, pic.1 + pic.3 - c.1 - c.3, 0.0, shade);
        p.fill_round_rect(pic.0, c.1, c.0 - pic.0, c.3, 0.0, shade);
        p.fill_round_rect(c.0 + c.2, c.1, pic.0 + pic.2 - c.0 - c.2, c.3, 0.0, shade);
        // Thirds, frame and handles.
        for k in 1..3 {
            let fx = c.0 + c.2 * k as f32 / 3.0;
            let fy = c.1 + c.3 * k as f32 / 3.0;
            p.line(fx, c.1, fx, c.1 + c.3, Rgba(1.0, 1.0, 1.0, 0.35), 1.0);
            p.line(c.0, fy, c.0 + c.2, fy, Rgba(1.0, 1.0, 1.0, 0.35), 1.0);
        }
        p.stroke_round_rect(c.0, c.1, c.2, c.3, 0.0, ACCENT, 2.0);
        for (hx, hy) in [(c.0, c.1), (c.0 + c.2, c.1), (c.0, c.1 + c.3), (c.0 + c.2, c.1 + c.3), (c.0 + c.2 / 2.0, c.1), (c.0 + c.2 / 2.0, c.1 + c.3), (c.0, c.1 + c.3 / 2.0), (c.0 + c.2, c.1 + c.3 / 2.0)] {
            p.fill_circle(hx, hy, 6.0, ACCENT);
            p.fill_circle(hx, hy, 2.5, Rgba::WHITE);
        }
    }
    fn canvas_down(&mut self, x: f32, y: f32, r: R) -> bool {
        let pic = self.picture(r);
        let c = self.screen(pic);
        let near = |a: f32, b: f32| (a - b).abs() <= 10.0;
        let inside_y = y >= c.1 - 10.0 && y <= c.1 + c.3 + 10.0;
        let inside_x = x >= c.0 - 10.0 && x <= c.0 + c.2 + 10.0;
        let (l, t, rt, b) = (near(x, c.0) && inside_y, near(y, c.1) && inside_x, near(x, c.0 + c.2) && inside_y, near(y, c.1 + c.3) && inside_x);
        let grab = if l || t || rt || b {
            Grab::Edges(l, t, rt, b)
        } else if contains(c, x, y) {
            Grab::Move
        } else {
            return false;
        };
        self.grab = Some((grab, (x, y), self.rect));
        true
    }
    fn canvas_move(&mut self, x: f32, y: f32, r: R, held: bool) -> bool {
        let Some((grab, (sx, sy), start)) = self.grab.filter(|_| held) else { return false };
        let pic = self.picture(r);
        let (dx, dy) = (((x - sx) / pic.2) as f64, ((y - sy) / pic.3) as f64);
        let (mut x0, mut y0, mut x1, mut y1) = (start.0, start.1, start.0 + start.2, start.1 + start.3);
        let min = 0.04;
        match grab {
            Grab::Move => {
                let (w, h) = (start.2, start.3);
                x0 = (start.0 + dx).clamp(0.0, 1.0 - w);
                y0 = (start.1 + dy).clamp(0.0, 1.0 - h);
                x1 = x0 + w;
                y1 = y0 + h;
            }
            Grab::Edges(l, t, rt, b) => {
                if l {
                    x0 = (start.0 + dx).clamp(0.0, x1 - min);
                }
                if rt {
                    x1 = (x1 + dx).clamp(x0 + min, 1.0);
                }
                if t {
                    y0 = (start.1 + dy).clamp(0.0, y1 - min);
                }
                if b {
                    y1 = (y1 + dy).clamp(y0 + min, 1.0);
                }
                let ratio = RATIOS[self.ratio].1;
                if ratio > 0.0 {
                    // Keep the ratio: the height follows the width.
                    let (vw, vh) = (self.size.0 as f64, self.size.1 as f64);
                    let h = ((x1 - x0) * vw / ratio) / vh;
                    if t && !b {
                        y0 = (y1 - h).max(0.0);
                    } else {
                        y1 = (y0 + h).min(1.0);
                    }
                    let w = ((y1 - y0) * vh * ratio) / vw;
                    if l && !rt {
                        x0 = x1 - w;
                    } else {
                        x1 = x0 + w;
                    }
                }
            }
        }
        self.rect = (x0, y0, x1 - x0, y1 - y0);
        true
    }
    fn canvas_up(&mut self, _x: f32, _y: f32, _r: R) -> bool {
        self.grab = None;
        true
    }
    fn fields(&self) -> Vec<Field> {
        let (w, h) = self.pixels();
        vec![
            Field::Choice { id: 1, label: "Ratio".into(), choices: RATIOS.iter().map(|r| r.0.to_string()).collect(), value: self.ratio },
            Field::Note(format!("{} × {} becomes {w} × {h}. Drag the box or its edges.", self.size.0, self.size.1)),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        if let (1, Change::Choice(i)) = (id, change) {
            self.ratio = i.min(RATIOS.len() - 1);
            if self.ratio > 0 {
                self.rect = (0.0, 0.0, 1.0, 1.0);
                self.apply_ratio();
            }
        }
    }
    fn action(&self) -> String {
        "Crop".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        let (x, y, w, h) = self.rect;
        if w > 0.995 && h > 0.995 {
            return Err("Drag the edges to choose what to keep.".into());
        }
        Ok(Operation::Configured(Tool::CropVideo, ToolOptions::CropVideo { x, y, w, h }))
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
