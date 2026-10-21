//! Organize PDF: the pages as a grid of thumbnails to reorder by dragging, rotate,
//! duplicate or delete.

use super::form::{contains, Change, Field, Form, R};
use super::gfx::{Align, Painter, Rgba, Weight};
use super::theme::{Palette, ACCENT};
use super::timeline::{fit, load_png, Image};
use satsuma_core::{Format, Tool};
use satsuma_engine::plan::ToolOptions;
use satsuma_engine::Operation;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use windows::Win32::UI::Input::KeyboardAndMouse::*;

const COLS: usize = 5;
const CARD_W: f32 = 92.0;
const CARD_H: f32 = 132.0;
const GAP: f32 = 12.0;

#[derive(Default)]
struct Thumbs {
    pages: Vec<Image>,
    done: bool,
    failed: bool,
}

pub struct Pages {
    files: Vec<PathBuf>,
    count: usize,
    thumbs: Arc<Mutex<Thumbs>>,
    /// The new order: source page and extra rotation.
    order: Vec<(usize, u16)>,
    selected: Option<usize>,
    /// A card being dragged: its index, where the pointer went down, and whether it moved.
    drag: Option<(usize, (f32, f32), bool)>,
    pointer: (f32, f32),
    scroll: f32,
}

impl Pages {
    pub fn new(files: Vec<PathBuf>) -> Option<Pages> {
        let path = files.first()?.clone();
        let count = satsuma_engine::pdf::page_count(&path).ok()?.max(1);
        let thumbs = Arc::new(Mutex::new(Thumbs::default()));
        let t = thumbs.clone();
        std::thread::spawn(move || {
            let dir = tempfile::tempdir();
            let rendered = dir.as_ref().ok().and_then(|d| {
                satsuma_engine::pdf::render_pages(&path, Format::Png, 24.0, &|i, _| d.path().join(format!("page-{i:04}.png")), &mut |_| {}).ok()
            });
            let images: Vec<Image> = rendered.unwrap_or_default().iter().filter_map(|p| load_png(p)).collect();
            if let Ok(mut t) = t.lock() {
                t.failed = images.is_empty();
                t.pages = images;
                t.done = true;
            }
        });
        Some(Pages { files, count, thumbs, order: (0..count).map(|i| (i, 0)).collect(), selected: None, drag: None, pointer: (0.0, 0.0), scroll: 0.0 })
    }

    fn origin(&self, r: R) -> (f32, f32) {
        let grid_w = COLS as f32 * CARD_W + (COLS - 1) as f32 * GAP;
        (r.0 + (r.2 - grid_w) / 2.0, r.1 + 10.0 - self.scroll)
    }

    fn card(&self, r: R, i: usize) -> R {
        let (ox, oy) = self.origin(r);
        let (c, row) = (i % COLS, i / COLS);
        (ox + c as f32 * (CARD_W + GAP), oy + row as f32 * (CARD_H + GAP), CARD_W, CARD_H)
    }

    fn index_at(&self, r: R, x: f32, y: f32) -> Option<usize> {
        (0..self.order.len()).find(|i| contains(self.card(r, *i), x, y))
    }

    /// Where a dragged card would land: the slot nearest the pointer.
    fn drop_slot(&self, r: R, x: f32, y: f32) -> usize {
        let (ox, oy) = self.origin(r);
        let col = (((x - ox) / (CARD_W + GAP)) + 0.5).floor().clamp(0.0, COLS as f32) as usize;
        let row = ((y - oy) / (CARD_H + GAP)).floor().max(0.0) as usize;
        (row * COLS + col).min(self.order.len())
    }

    fn content_height(&self) -> f32 {
        let rows = self.order.len().div_ceil(COLS);
        rows as f32 * (CARD_H + GAP) + 10.0
    }

    fn act(&mut self, action: usize) {
        let Some(i) = self.selected.filter(|i| *i < self.order.len()) else { return };
        match action {
            0 => self.order[i].1 = (self.order[i].1 + 270) % 360,
            1 => self.order[i].1 = (self.order[i].1 + 90) % 360,
            2 => {
                let copy = self.order[i];
                self.order.insert(i + 1, copy);
                self.selected = Some(i + 1);
            }
            _ => {
                if self.order.len() > 1 {
                    self.order.remove(i);
                    self.selected = Some(i.min(self.order.len() - 1));
                }
            }
        }
    }
}

/// `img` turned clockwise by `degrees` (0, 90, 180, 270).
fn rotated(img: &Image, degrees: u16) -> Option<Image> {
    if degrees == 0 {
        return None;
    }
    let buf = image::RgbaImage::from_raw(img.w, img.h, img.rgba.clone())?;
    let out = match degrees {
        90 => image::imageops::rotate90(&buf),
        180 => image::imageops::rotate180(&buf),
        _ => image::imageops::rotate270(&buf),
    };
    Some(Image { w: out.width(), h: out.height(), rgba: out.into_raw() })
}

impl Form for Pages {
    fn title(&self) -> String {
        "Organize PDF".into()
    }
    fn width(&self) -> f32 {
        COLS as f32 * CARD_W + (COLS - 1) as f32 * GAP + 2.0 * 18.0 + 24.0
    }
    fn canvas_height(&self) -> f32 {
        (CARD_H + GAP) * 2.6 + 10.0
    }
    fn paint_canvas(&self, p: &Painter, r: R, pal: &Palette) {
        p.fill_round_rect(r.0, r.1, r.2, r.3, 12.0, if pal.glass { Rgba(0.0, 0.0, 0.0, 0.22) } else { pal.control });
        let thumbs = self.thumbs.lock();
        let thumbs = thumbs.as_ref().ok();
        let dragging = self.drag.filter(|d| d.2).map(|d| d.0);
        for (i, (src, rot)) in self.order.iter().enumerate() {
            let c = self.card(r, i);
            if c.1 + c.3 < r.1 || c.1 > r.1 + r.3 {
                continue;
            }
            // Cards are clipped by not drawing those outside; partly visible ones show whole.
            let faded = dragging == Some(i);
            let area = (c.0 + 4.0, c.1 + 4.0, c.2 - 8.0, c.3 - 26.0);
            p.fill_round_rect(c.0, c.1, c.2, c.3, 10.0, if self.selected == Some(i) { ACCENT.alpha(0.22) } else { Rgba(1.0, 1.0, 1.0, if pal.glass { 0.06 } else { 0.5 }) });
            if let Some(img) = thumbs.and_then(|t| t.pages.get(*src)) {
                let turned = rotated(img, *rot);
                let img = turned.as_ref().unwrap_or(img);
                let f = fit(img.w, img.h, area);
                p.fill_round_rect(f.0, f.1, f.2, f.3, 2.0, Rgba::WHITE);
                p.image(&img.rgba, img.w, img.h, f);
                if faded {
                    p.fill_round_rect(f.0, f.1, f.2, f.3, 2.0, Rgba(0.0, 0.0, 0.0, 0.5));
                }
            } else {
                p.fill_round_rect(area.0 + 14.0, area.1 + 6.0, area.2 - 28.0, area.3 - 12.0, 2.0, Rgba(1.0, 1.0, 1.0, 0.85));
            }
            if self.selected == Some(i) {
                p.stroke_round_rect(c.0 + 1.0, c.1 + 1.0, c.2 - 2.0, c.3 - 2.0, 9.0, ACCENT, 2.0);
            }
            let label = if *rot == 0 { format!("{}", src + 1) } else { format!("{} · {}°", src + 1, rot) };
            p.text(&label, (c.0, c.1 + c.3 - 22.0, c.2, 18.0), 11.0, Weight::Semibold, pal.secondary, Align::Center, false);
        }
        // The dragged card follows the pointer; a bar marks where it will go.
        if let Some((from, _, true)) = self.drag {
            let slot = self.drop_slot(r, self.pointer.0, self.pointer.1);
            let target = if slot < self.order.len() { self.card(r, slot) } else { let c = self.card(r, self.order.len() - 1); (c.0 + c.2 + GAP, c.1, c.2, c.3) };
            p.fill_round_rect(target.0 - GAP / 2.0 - 2.0, target.1, 4.0, target.3, 2.0, ACCENT);
            let (src, rot) = self.order[from];
            let gx = (self.pointer.0 - CARD_W / 2.0).clamp(r.0, r.0 + r.2 - CARD_W);
            let gy = (self.pointer.1 - CARD_H / 2.0).clamp(r.1, r.1 + r.3 - CARD_H);
            let ghost = (gx, gy, CARD_W, CARD_H);
            p.fill_round_rect(ghost.0, ghost.1, ghost.2, ghost.3, 10.0, ACCENT.alpha(0.3));
            if let Some(img) = thumbs.and_then(|t| t.pages.get(src)) {
                let turned = rotated(img, rot);
                let img = turned.as_ref().unwrap_or(img);
                p.image(&img.rgba, img.w, img.h, fit(img.w, img.h, (ghost.0 + 4.0, ghost.1 + 4.0, ghost.2 - 8.0, ghost.3 - 26.0)));
            }
        }
        if thumbs.is_some_and(|t| !t.done) {
            p.text("Loading pages…", (r.0, r.1 + r.3 - 24.0, r.2, 18.0), 11.0, Weight::Regular, pal.tertiary, Align::Center, false);
        }
    }
    fn canvas_down(&mut self, x: f32, y: f32, r: R) -> bool {
        self.pointer = (x, y);
        match self.index_at(r, x, y) {
            Some(i) => {
                self.selected = Some(i);
                self.drag = Some((i, (x, y), false));
            }
            None => self.selected = None,
        }
        true
    }
    fn canvas_move(&mut self, x: f32, y: f32, _r: R, held: bool) -> bool {
        self.pointer = (x, y);
        if let (true, Some((i, (sx, sy), moved))) = (held, self.drag) {
            let now = moved || (x - sx).abs() + (y - sy).abs() > 6.0;
            self.drag = Some((i, (sx, sy), now));
            return now;
        }
        false
    }
    fn canvas_up(&mut self, x: f32, y: f32, r: R) -> bool {
        if let Some((from, _, true)) = self.drag.take() {
            let slot = self.drop_slot(r, x, y);
            let item = self.order.remove(from);
            let to = if slot > from { slot - 1 } else { slot }.min(self.order.len());
            self.order.insert(to, item);
            self.selected = Some(to);
        }
        self.drag = None;
        true
    }
    fn canvas_wheel(&mut self, delta: f32) -> bool {
        let max = (self.content_height() - self.canvas_height()).max(0.0);
        self.scroll = (self.scroll + delta * 60.0).clamp(0.0, max);
        true
    }
    fn key(&mut self, vk: VIRTUAL_KEY) -> bool {
        match vk {
            VK_DELETE | VK_BACK => self.act(3),
            VK_LEFT => self.selected = self.selected.map(|i| i.saturating_sub(1)),
            VK_RIGHT => self.selected = self.selected.map(|i| (i + 1).min(self.order.len() - 1)),
            _ => return false,
        }
        true
    }
    fn fields(&self) -> Vec<Field> {
        let failed = self.thumbs.lock().map(|t| t.failed).unwrap_or(false);
        vec![
            Field::Choice { id: 1, label: "".into(), choices: vec!["Rotate Left".into(), "Rotate Right".into(), "Duplicate".into(), "Delete".into()], value: usize::MAX },
            Field::Note(if failed {
                "The pages couldn't be shown, but you can still reorder them.".into()
            } else {
                format!("{} pages of {}. Click a page, then rotate, duplicate or delete it. Drag pages to reorder.", self.order.len(), self.count)
            }),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        if let (1, Change::Choice(i)) = (id, change) {
            self.act(i);
        }
    }
    fn action(&self) -> String {
        "Save PDF".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        let unchanged = self.order.len() == self.count && self.order.iter().enumerate().all(|(i, (s, r))| i == *s && *r == 0);
        if unchanged {
            return Err("Reorder, rotate, duplicate or delete a page first.".into());
        }
        Ok(Operation::Configured(Tool::OrganizePdf, ToolOptions::Pages { pages: self.order.clone() }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
    fn animating(&self) -> bool {
        self.thumbs.lock().map(|t| !t.done).unwrap_or(false)
    }
}
