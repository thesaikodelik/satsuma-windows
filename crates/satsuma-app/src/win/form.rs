//! The window a tool opens for its choices, in Satsuma's look: a canvas when the tool needs
//! one (a timeline, a picture, pages), rows of controls, and Cancel / the tool's button.
//! Each tool describes itself through [`Form`]; this module draws and runs the window.

use super::editor::apply_theme;
use super::events::{self, UiEvent};
use super::gfx::{Align, Painter, Rgba, Surface, Weight};
use super::theme::{Palette, ACCENT};
use super::window::{self, Handler};
use super::system;
use satsuma_engine::Operation;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub type R = (f32, f32, f32, f32);

pub fn contains(r: R, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

/// One row of the window.
#[derive(Clone, Debug)]
pub enum Field {
    /// A choice among a few, as a segmented control.
    Choice { id: u32, label: String, choices: Vec<String>, value: usize },
    /// A number on a slider, with its value written beside it.
    Slider { id: u32, label: String, min: f64, max: f64, value: f64, text: String },
    Toggle { id: u32, label: String, value: bool },
    Text { id: u32, label: String, value: String, placeholder: String },
    /// A row of colours to pick from.
    Colours { id: u32, label: String, colours: Vec<[u8; 3]>, value: usize },
    /// A line of explanation.
    Note(String),
}

impl Field {
    fn id(&self) -> Option<u32> {
        match self {
            Field::Choice { id, .. } | Field::Slider { id, .. } | Field::Toggle { id, .. } | Field::Text { id, .. } | Field::Colours { id, .. } => Some(*id),
            Field::Note(_) => None,
        }
    }

    fn height(&self) -> f32 {
        match self {
            Field::Note(_) => 34.0,
            _ => 40.0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Change {
    Choice(usize),
    Slider(f64),
    Toggle(bool),
    Text(String),
    Colour(usize),
}

/// What a tool's window shows and does.
pub trait Form {
    fn title(&self) -> String;
    fn fields(&self) -> Vec<Field>;
    fn change(&mut self, id: u32, change: Change);
    /// The main button's title, e.g. "Save Clips".
    fn action(&self) -> String;
    /// The operation to run, or what's wrong.
    fn operation(&self) -> Result<Operation, String>;
    fn files(&self) -> Vec<PathBuf>;
    fn width(&self) -> f32 {
        460.0
    }
    /// A drawing area above the controls (0 for none).
    fn canvas_height(&self) -> f32 {
        0.0
    }
    fn paint_canvas(&self, _p: &Painter, _r: R, _pal: &Palette) {}
    /// Pointer events in the canvas; return true to redraw.
    fn canvas_down(&mut self, _x: f32, _y: f32, _r: R) -> bool {
        false
    }
    fn canvas_move(&mut self, _x: f32, _y: f32, _r: R, _held: bool) -> bool {
        false
    }
    fn canvas_up(&mut self, _x: f32, _y: f32, _r: R) -> bool {
        false
    }
    /// The mouse wheel over the canvas (positive = towards the user); return true to redraw.
    fn canvas_wheel(&mut self, _delta: f32) -> bool {
        false
    }
    /// Keys while no text field has focus; return true when handled.
    fn key(&mut self, _vk: VIRTUAL_KEY) -> bool {
        false
    }
    /// Still loading or moving: keep redrawing.
    fn animating(&self) -> bool {
        false
    }
    /// Called on each frame while animating.
    fn tick(&mut self) {}
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Field(u32, usize),
    Canvas,
    Cancel,
    Apply,
}

struct State {
    form: Box<dyn Form>,
    focus: Option<u32>,
    dragging: Option<u32>,
    pressed: Option<Hit>,
    canvas_held: bool,
    /// The first half of a character typed beyond the basic plane (an emoji, say).
    high_surrogate: Option<u16>,
    error: Option<String>,
    scale: f32,
}

const PAD: f32 = 18.0;
const LABEL_W: f32 = 150.0;
const FOOTER: f32 = 62.0;

impl State {
    fn width(&self) -> f32 {
        self.form.width()
    }

    fn canvas(&self) -> Option<R> {
        let h = self.form.canvas_height();
        (h > 0.0).then_some((PAD, PAD, self.width() - 2.0 * PAD, h))
    }

    fn content_height(&self) -> f32 {
        let mut y = PAD + self.canvas().map(|c| c.3 + 14.0).unwrap_or(0.0);
        for f in self.form.fields() {
            y += f.height();
        }
        y + 8.0 + FOOTER
    }

    /// Each field with its row rectangle.
    fn rows(&self) -> Vec<(Field, R)> {
        let w = self.width();
        let mut y = PAD + self.canvas().map(|c| c.3 + 14.0).unwrap_or(0.0);
        self.form
            .fields()
            .into_iter()
            .map(|f| {
                let h = f.height();
                let r = (PAD, y, w - 2.0 * PAD, h);
                y += h;
                (f, r)
            })
            .collect()
    }

    /// Where a field's control sits within its row.
    fn control(&self, f: &Field, row: R) -> R {
        let has_label = match f {
            Field::Choice { label, .. } | Field::Slider { label, .. } | Field::Toggle { label, .. } | Field::Text { label, .. } | Field::Colours { label, .. } => !label.is_empty(),
            Field::Note(_) => false,
        };
        let x = if has_label { row.0 + LABEL_W } else { row.0 };
        let w = row.0 + row.2 - x;
        match f {
            Field::Toggle { .. } => (row.0 + row.2 - 44.0, row.1 + 8.0, 44.0, 24.0),
            Field::Slider { .. } => (x, row.1 + 6.0, w - 64.0, 28.0),
            _ => (x, row.1 + 5.0, w, 30.0),
        }
    }

    fn buttons(&self) -> (R, R) {
        let w = self.width();
        let y = self.content_height() - FOOTER + 14.0;
        let aw = 160.0f32.max(16.0 + self.form.action().chars().count() as f32 * 7.6);
        let apply = (w - PAD - aw, y, aw, 34.0);
        let cancel = (apply.0 - 12.0 - 100.0, y, 100.0, 34.0);
        (cancel, apply)
    }

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let (cancel, apply) = self.buttons();
        if contains(apply, x, y) {
            return Some(Hit::Apply);
        }
        if contains(cancel, x, y) {
            return Some(Hit::Cancel);
        }
        if let Some(c) = self.canvas() {
            if contains(c, x, y) {
                return Some(Hit::Canvas);
            }
        }
        for (f, row) in self.rows() {
            let Some(id) = f.id() else { continue };
            let c = self.control(&f, row);
            if !contains((c.0 - 6.0, c.1 - 4.0, c.2 + 12.0, c.3 + 8.0), x, y) {
                continue;
            }
            let part = match &f {
                Field::Choice { choices, .. } => (((x - c.0) / c.2 * choices.len() as f32) as usize).min(choices.len().saturating_sub(1)),
                Field::Colours { colours, .. } => (((x - c.0) / 34.0) as usize).min(colours.len().saturating_sub(1)),
                _ => 0,
            };
            return Some(Hit::Field(id, part));
        }
        None
    }

    fn slider_value(&self, id: u32, x: f32) -> Option<f64> {
        for (f, row) in self.rows() {
            if let Field::Slider { id: fid, min, max, .. } = &f {
                if *fid == id {
                    let c = self.control(&f, row);
                    let t = ((x - c.0 - 8.0) / (c.2 - 16.0)).clamp(0.0, 1.0) as f64;
                    return Some(min + (max - min) * t);
                }
            }
        }
        None
    }

    fn text_of(&self, id: u32) -> Option<String> {
        self.form.fields().into_iter().find_map(|f| match f {
            Field::Text { id: fid, value, .. } if fid == id => Some(value),
            _ => None,
        })
    }

    fn paint(&self, p: &Painter, pal: &Palette) {
        let w = self.width();
        let h = self.content_height();
        p.fill_round_rect(0.0, 0.0, w, h, 0.0, if pal.glass { pal.backdrop } else { Rgba(pal.panel.0, pal.panel.1, pal.panel.2, 1.0) });
        if let Some(c) = self.canvas() {
            self.form.paint_canvas(p, c, pal);
        }
        for (f, row) in self.rows() {
            self.paint_field(p, pal, &f, row);
        }
        // Footer.
        let (cancel, apply) = self.buttons();
        if let Some(e) = &self.error {
            p.text(e, (PAD, cancel.1 - 2.0, cancel.0 - PAD - 8.0, 38.0), 11.0, Weight::Regular, pal.error, Align::Leading, true);
        }
        let radius = |r: R| if pal.glass { r.3 / 2.0 } else { 6.0 };
        let down = |hit: Hit| self.pressed == Some(hit);
        p.fill_round_rect(cancel.0, cancel.1, cancel.2, cancel.3, radius(cancel), pal.control.alpha(if down(Hit::Cancel) { 2.2 } else if pal.glass { 1.4 } else { 1.0 }));
        p.stroke_round_rect(cancel.0 + 0.5, cancel.1 + 0.5, cancel.2 - 1.0, cancel.3 - 1.0, radius(cancel) - 0.5, if pal.glass { pal.rim } else { pal.separator }, 1.0);
        p.text("Cancel", cancel, 12.5, Weight::Semibold, pal.label, Align::Center, false);
        p.fill_round_rect(apply.0, apply.1, apply.2, apply.3, radius(apply), if down(Hit::Apply) { Rgba(0.85, 0.36, 0.07, 1.0) } else { ACCENT });
        p.text(&self.form.action(), apply, 12.5, Weight::Semibold, Rgba::WHITE, Align::Center, false);
    }

    fn paint_field(&self, p: &Painter, pal: &Palette, f: &Field, row: R) {
        let label = |text: &str| p.text(text, (row.0, row.1, LABEL_W - 8.0, row.3), 12.5, Weight::Regular, pal.label, Align::Leading, false);
        let c = self.control(f, row);
        match f {
            Field::Note(text) => p.text(text, (row.0, row.1 + 2.0, row.2, row.3 - 4.0), 11.0, Weight::Regular, pal.tertiary, Align::Leading, true),
            Field::Choice { label: l, choices, value, .. } => {
                if !l.is_empty() {
                    label(l);
                }
                let r = c.3 / 2.0;
                p.fill_round_rect(c.0, c.1, c.2, c.3, r, if pal.glass { pal.control } else { pal.separator.alpha(0.6) });
                if pal.glass {
                    p.stroke_round_rect(c.0 + 0.5, c.1 + 0.5, c.2 - 1.0, c.3 - 1.0, r - 0.5, pal.rim, 1.0);
                }
                let n = choices.len().max(1) as f32;
                let seg = c.2 / n;
                if *value < choices.len() {
                    let sx = c.0 + seg * *value as f32;
                    p.fill_round_rect(sx + 3.0, c.1 + 3.0, seg - 6.0, c.3 - 6.0, r - 3.0, if pal.glass { Rgba(1.0, 1.0, 1.0, 0.16) } else { pal.panel.alpha(1.0) });
                }
                for (i, name) in choices.iter().enumerate() {
                    let on = i == *value;
                    p.text(name, (c.0 + seg * i as f32, c.1, seg, c.3), 12.0, if on { Weight::Semibold } else { Weight::Regular }, if on { pal.label } else { pal.secondary }, Align::Center, false);
                }
            }
            Field::Slider { label: l, value, min, max, text, .. } => {
                label(l);
                let t = if max > min { ((value - min) / (max - min)).clamp(0.0, 1.0) as f32 } else { 0.0 };
                let (x0, x1, cy) = (c.0 + 8.0, c.0 + c.2 - 8.0, c.1 + c.3 / 2.0);
                p.fill_round_rect(x0, cy - 2.0, x1 - x0, 4.0, 2.0, pal.control.alpha(if pal.glass { 2.0 } else { 1.0 }));
                p.fill_round_rect(x0, cy - 2.0, (x1 - x0) * t, 4.0, 2.0, ACCENT);
                let kx = x0 + (x1 - x0) * t;
                p.fill_circle(kx, cy, 9.0, if pal.glass { Rgba(1.0, 1.0, 1.0, 0.95) } else { Rgba::WHITE });
                p.stroke_circle(kx, cy, 9.0, pal.separator, 1.0);
                p.text(text, (c.0 + c.2 + 6.0, row.1, 58.0, row.3), 12.0, Weight::Regular, pal.secondary, Align::Leading, false);
            }
            Field::Toggle { label: l, value, .. } => {
                label(l);
                let r = c.3 / 2.0;
                if *value {
                    p.fill_round_rect(c.0, c.1, c.2, c.3, r, ACCENT);
                } else {
                    p.fill_round_rect(c.0, c.1, c.2, c.3, r, pal.control.alpha(if pal.glass { 1.6 } else { 1.0 }));
                    p.stroke_round_rect(c.0 + 0.5, c.1 + 0.5, c.2 - 1.0, c.3 - 1.0, r - 0.5, if pal.glass { pal.rim } else { pal.tertiary }, 1.0);
                }
                let kx = if *value { c.0 + c.2 - r } else { c.0 + r };
                p.fill_circle(kx, c.1 + r, r - 4.0, if *value { Rgba::WHITE } else { pal.secondary });
            }
            Field::Text { id, label: l, value, placeholder } => {
                label(l);
                let focused = self.focus == Some(*id);
                p.fill_round_rect(c.0, c.1, c.2, c.3, 7.0, pal.control.alpha(if pal.glass { 1.5 } else { 1.0 }));
                p.stroke_round_rect(c.0 + 0.5, c.1 + 0.5, c.2 - 1.0, c.3 - 1.0, 6.5, if focused { ACCENT } else if pal.glass { pal.rim } else { pal.separator }, if focused { 1.6 } else { 1.0 });
                let inner = (c.0 + 10.0, c.1, c.2 - 20.0, c.3);
                if value.is_empty() && !focused {
                    p.text(placeholder, inner, 12.0, Weight::Regular, pal.tertiary, Align::Leading, false);
                } else {
                    p.text(value, inner, 12.0, Weight::Regular, pal.label, Align::Leading, false);
                }
                if focused {
                    let x = inner.0 + p.text_width(value, 12.0, Weight::Regular).min(inner.2);
                    p.line(x + 1.0, c.1 + 7.0, x + 1.0, c.1 + c.3 - 7.0, pal.label, 1.2);
                }
            }
            Field::Colours { label: l, colours, value, .. } => {
                label(l);
                for (i, rgb) in colours.iter().enumerate() {
                    let x = c.0 + i as f32 * 34.0;
                    p.fill_round_rect(x, c.1 + 2.0, 26.0, 26.0, 7.0, Rgba(rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, 1.0));
                    p.stroke_round_rect(x, c.1 + 2.0, 26.0, 26.0, 7.0, pal.separator, 0.6);
                    if i == *value {
                        p.stroke_round_rect(x + 1.5, c.1 + 3.5, 23.0, 23.0, 6.0, ACCENT, 3.0);
                    }
                }
            }
        }
    }
}

struct Messages {
    state: Rc<RefCell<State>>,
}

const TIMER: usize = 1;

fn scale_of(hwnd: HWND) -> f32 {
    (unsafe { GetDpiForWindow(hwnd) }.max(96)) as f32 / 96.0
}

fn refresh(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn close(hwnd: HWND) {
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

impl Messages {
    fn apply(&self, hwnd: HWND) {
        let result = {
            let s = self.state.borrow();
            s.form.operation().map(|op| (op, s.form.files()))
        };
        match result {
            Ok((op, files)) => {
                events::post(UiEvent::EditorApply { op, files, at: system::cursor_pos() });
                close(hwnd);
            }
            Err(e) => {
                self.state.borrow_mut().error = Some(e);
                refresh(hwnd);
            }
        }
    }

    fn resize_to_content(&self, hwnd: HWND) {
        let (w, h) = {
            let s = self.state.borrow();
            (s.width(), s.content_height())
        };
        let scale = scale_of(hwnd);
        let style = WINDOW_STYLE(unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32);
        let mut r = RECT { left: 0, top: 0, right: (w * scale).round() as i32, bottom: (h * scale).round() as i32 };
        unsafe {
            let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), GetDpiForWindow(hwnd));
            let mut now = RECT::default();
            let _ = GetWindowRect(hwnd, &mut now);
            if now.bottom - now.top != r.bottom - r.top {
                let _ = SetWindowPos(hwnd, None, 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }
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
                    let scale = scale_of(hwnd);
                    if let Ok(mut s) = self.state.try_borrow_mut() {
                        s.scale = scale;
                    }
                    if let Ok(s) = self.state.try_borrow() {
                        let pal = Palette::current();
                        Surface::with(r.right, r.bottom, |surface| {
                            surface.draw(scale, |p| s.paint(p, &pal));
                            surface.blit(dc, 0, 0);
                        });
                    }
                    let _ = EndPaint(hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_TIMER if wparam.0 == TIMER => {
                let animating = {
                    let Ok(mut s) = self.state.try_borrow_mut() else { return Some(LRESULT(0)) };
                    s.form.tick();
                    s.form.animating()
                };
                if !animating {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER);
                    }
                }
                self.resize_to_content(hwnd);
                refresh(hwnd);
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                let (x, y) = dip(lparam);
                {
                    let mut s = self.state.try_borrow_mut().ok()?;
                    s.error = None;
                    let hit = s.hit(x, y);
                    s.pressed = hit;
                    s.focus = None;
                    match hit {
                        Some(Hit::Canvas) => {
                            let c = s.canvas().unwrap();
                            s.canvas_held = true;
                            s.form.canvas_down(x, y, c);
                        }
                        Some(Hit::Field(id, part)) => {
                            let field = s.form.fields().into_iter().find(|f| f.id() == Some(id));
                            match field {
                                Some(Field::Choice { .. }) => s.form.change(id, Change::Choice(part)),
                                Some(Field::Colours { .. }) => s.form.change(id, Change::Colour(part)),
                                Some(Field::Toggle { value, .. }) => s.form.change(id, Change::Toggle(!value)),
                                Some(Field::Slider { .. }) => {
                                    s.dragging = Some(id);
                                    if let Some(v) = s.slider_value(id, x) {
                                        s.form.change(id, Change::Slider(v));
                                    }
                                }
                                Some(Field::Text { .. }) => s.focus = Some(id),
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                unsafe { SetCapture(hwnd) };
                self.resize_to_content(hwnd);
                refresh(hwnd);
                self.animate(hwnd);
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = dip(lparam);
                let mut s = self.state.try_borrow_mut().ok()?;
                let mut redraw = false;
                if let Some(id) = s.dragging {
                    if let Some(v) = s.slider_value(id, x) {
                        s.form.change(id, Change::Slider(v));
                        redraw = true;
                    }
                } else if let Some(c) = s.canvas() {
                    let held = s.canvas_held;
                    if held || contains(c, x, y) {
                        redraw = s.form.canvas_move(x, y, c, held);
                    }
                }
                drop(s);
                if redraw {
                    refresh(hwnd);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                let (x, y) = dip(lparam);
                let clicked = {
                    let mut s = self.state.try_borrow_mut().ok()?;
                    s.dragging = None;
                    if s.canvas_held {
                        s.canvas_held = false;
                        if let Some(c) = s.canvas() {
                            s.form.canvas_up(x, y, c);
                        }
                    }
                    let pressed = s.pressed.take();
                    pressed.filter(|p| s.hit(x, y) == Some(*p))
                };
                match clicked {
                    Some(Hit::Cancel) => close(hwnd),
                    Some(Hit::Apply) => self.apply(hwnd),
                    _ => {
                        self.resize_to_content(hwnd);
                        refresh(hwnd);
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = -(window::hiword(wparam.0 as isize) as f32) / 120.0;
                let redraw = self.state.try_borrow_mut().map(|mut s| s.form.canvas_wheel(delta)).unwrap_or(false);
                if redraw {
                    refresh(hwnd);
                }
                Some(LRESULT(0))
            }
            WM_CHAR => {
                let code = wparam.0 as u32;
                let mut s = self.state.try_borrow_mut().ok()?;
                // Emoji and other characters beyond the basic plane come as two UTF-16 halves.
                if (0xD800..0xDC00).contains(&code) {
                    s.high_surrogate = Some(code as u16);
                    return Some(LRESULT(0));
                }
                let ch = if (0xDC00..0xE000).contains(&code) {
                    let high = s.high_surrogate.take()?;
                    char::decode_utf16([high, code as u16]).next()?.ok()?
                } else {
                    char::from_u32(code)?
                };
                let Some(id) = s.focus else { return Some(LRESULT(0)) };
                let mut text = s.text_of(id).unwrap_or_default();
                match ch {
                    '\u{8}' => {
                        text.pop();
                    }
                    '\r' | '\n' | '\u{1b}' | '\t' => return Some(LRESULT(0)),
                    c if !c.is_control() && text.chars().count() < 200 => text.push(c),
                    _ => return Some(LRESULT(0)),
                }
                s.form.change(id, Change::Text(text));
                drop(s);
                refresh(hwnd);
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                if vk == VK_ESCAPE {
                    close(hwnd);
                } else if vk == VK_RETURN {
                    self.apply(hwnd);
                } else if vk == VK_TAB {
                    let mut s = self.state.borrow_mut();
                    let texts: Vec<u32> = s.form.fields().iter().filter(|f| matches!(f, Field::Text { .. })).filter_map(|f| f.id()).collect();
                    if !texts.is_empty() {
                        let next = s.focus.and_then(|f| texts.iter().position(|t| *t == f)).map(|i| (i + 1) % texts.len()).unwrap_or(0);
                        s.focus = Some(texts[next]);
                    }
                    drop(s);
                    refresh(hwnd);
                } else {
                    let handled = {
                        let mut s = self.state.borrow_mut();
                        s.focus.is_none() && s.form.key(vk)
                    };
                    if handled {
                        refresh(hwnd);
                        self.animate(hwnd);
                    }
                }
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                let r = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                self.resize_to_content(hwnd);
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                apply_theme(hwnd);
                refresh(hwnd);
                None
            }
            WM_DESTROY => {
                events::post(UiEvent::WindowClosed { window: hwnd.0 as isize });
                None
            }
            _ => None,
        }
    }
}

impl Messages {
    fn animate(&self, hwnd: HWND) {
        if self.state.try_borrow().map(|s| s.form.animating()).unwrap_or(false) {
            unsafe { SetTimer(Some(hwnd), TIMER, 16, None) };
        }
    }
}

/// Renders a form to a PNG at 2× on a plain backdrop, after applying `sets` as if chosen
/// (for `satsuma render-form`): each value goes to the field with that id, as its kind needs.
pub fn render_png(mut form: Box<dyn Form>, sets: &[(u32, f64)], pal: &Palette, out: &std::path::Path) -> Result<(), String> {
    for (id, v) in sets {
        let field = form.fields().into_iter().find(|f| f.id() == Some(*id));
        let change = match field {
            Some(Field::Choice { .. }) => Change::Choice(*v as usize),
            Some(Field::Colours { .. }) => Change::Colour(*v as usize),
            Some(Field::Toggle { .. }) => Change::Toggle(*v != 0.0),
            Some(Field::Text { .. }) => Change::Text(v.to_string()),
            _ => Change::Slider(*v),
        };
        form.change(*id, change);
    }
    for _ in 0..600 {
        if !form.animating() {
            break;
        }
        form.tick();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let state = State { form, focus: None, dragging: None, pressed: None, canvas_held: false, high_surrogate: None, error: None, scale: 2.0 };
    let (w, h) = (state.width(), state.content_height());
    let scale = 2.0;
    let (pw, ph) = ((w * scale).ceil() as i32, (h * scale).ceil() as i32);
    let surface = Surface::new(pw, ph).map_err(|e| e.message())?;
    surface.draw(scale, |p| {
        p.fill_round_rect(0.0, 0.0, w, h, 0.0, if pal.dark { Rgba(0.13, 0.13, 0.15, 1.0) } else { Rgba(0.93, 0.93, 0.94, 1.0) });
        state.paint(p, pal);
    });
    super::gfx::save_png(&surface.rgba(), pw as u32, ph as u32, out)
}

/// An open tool window.
pub struct FormWindow {
    hwnd: HWND,
}

impl FormWindow {
    pub fn open(form: Box<dyn Form>) -> FormWindow {
        let title = form.title();
        let state = Rc::new(RefCell::new(State { form, focus: None, dragging: None, pressed: None, canvas_held: false, high_surrogate: None, error: None, scale: 1.0 }));
        let (w, h) = {
            let s = state.borrow();
            (s.width(), s.content_height())
        };
        let (cx, cy) = system::cursor_pos();
        let scale = system::scale_at(cx, cy);
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let ex = WINDOW_EX_STYLE(0);
        let mut r = RECT { left: 0, top: 0, right: (w * scale).round() as i32, bottom: (h * scale).round() as i32 };
        unsafe {
            let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, (scale * 96.0) as u32);
        }
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let work = system::work_area_at(cx, cy);
        let x = (work.left + work.right - ww) / 2;
        let y = ((work.top + work.bottom - wh) / 2).max(work.top);
        let messages = Rc::new(Messages { state });
        let hwnd = window::create(ex, style, &title, x, y, ww, wh, None, messages.clone());
        apply_theme(hwnd);
        messages.resize_to_content(hwnd);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
        messages.animate(hwnd);
        FormWindow { hwnd }
    }

    pub fn id(&self) -> isize {
        self.hwnd.0 as isize
    }
}
