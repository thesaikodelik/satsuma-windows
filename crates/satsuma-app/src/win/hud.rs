//! The small progress panel near the drop point: title, "k of n saved", a progress bar and
//! a × that cancels the job. Click-through except for the ×.

use super::events::{self, Point, UiEvent};
use super::gfx::{Align, Painter, Surface, Weight};
use super::theme::{Palette, ACCENT};
use super::window::{self, Handler};
use super::system;
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WIDTH: f32 = 300.0;
pub const HEIGHT: f32 = 62.0;
/// Room around the panel for its shadow.
const MARGIN: f32 = 10.0;

#[derive(Clone, Debug, Default)]
pub struct HudModel {
    pub title: String,
    pub subtitle: String,
    pub fraction: Option<f64>,
    pub error: bool,
}

struct State {
    model: HudModel,
    origin: Point,
    scale: f32,
    visible: bool,
    behind: Option<(Vec<u8>, u32, u32)>,
}

pub struct Hud {
    hwnd: HWND,
    state: Rc<RefCell<State>>,
}

fn paint(p: &Painter, m: &HudModel, pal: &Palette, behind: Option<&(Vec<u8>, u32, u32)>) {
    let (x, y) = (MARGIN, MARGIN);
    if let (true, Some((px, w, h))) = (pal.glass, behind) {
        p.fill_round_rect_image((x, y, WIDTH, HEIGHT), 14.0, (0.0, 0.0, WIDTH + MARGIN * 2.0, HEIGHT + MARGIN * 2.0), px, *w, *h);
    }
    if pal.glass {
        // 30nama's drawn glass: tint, faint body, glare from above, lit rim.
        let white = |a: f32| super::gfx::Rgba(1.0, 1.0, 1.0, a);
        p.fill_round_rect(x, y, WIDTH, HEIGHT, 14.0, pal.panel);
        p.fill_round_rect(x, y, WIDTH, HEIGHT, 14.0, white(0.02));
        let glare = [(0.0, white(0.12)), (0.5, white(0.02)), (0.7, white(0.0)), (1.0, white(0.04))];
        p.round_rect_gradient(x, y, WIDTH, HEIGHT, 14.0, &glare, (x, y), (x, y + HEIGHT), None);
        let rim = [(0.0, white(0.55)), (1.0 / 3.0, white(0.12)), (2.0 / 3.0, white(0.05)), (1.0, white(0.22))];
        p.round_rect_gradient(x + 0.6, y + 0.6, WIDTH - 1.2, HEIGHT - 1.2, 13.4, &rim, (x, y), (x, y + HEIGHT), Some(1.2));
    } else {
        // Soft shadow, then the panel.
        for i in 1..=4 {
            let f = i as f32 * 1.6;
            p.fill_round_rect(x - f * 0.5, y - f * 0.25, WIDTH + f, HEIGHT + f, 14.0 + f * 0.5, super::gfx::Rgba(0.0, 0.0, 0.0, 0.035));
        }
        p.fill_round_rect(x, y, WIDTH, HEIGHT, 14.0, pal.panel);
        p.stroke_round_rect(x + 0.25, y + 0.25, WIDTH - 0.5, HEIGHT - 0.5, 14.0, pal.separator, 0.5);
    }

    // Close button.
    p.fill_circle(x + 20.0, y + 20.0, 9.0, pal.tertiary.alpha(0.35));
    p.text_centered("×", x + 20.0, y + 19.5, 12.0, Weight::Semibold, pal.secondary);

    let title_color = if m.error { pal.error } else { pal.label };
    p.text(&m.title, (x + 38.0, y + 11.0, WIDTH - 52.0, 18.0), 13.0, Weight::Semibold, title_color, Align::Leading, false);
    let bar = m.fraction.is_some();
    let sh = if bar { 15.0 } else { 30.0 };
    p.text(&m.subtitle, (x + 14.0, y + 31.0, WIDTH - 28.0, sh), 11.0, Weight::Regular, pal.secondary, Align::Leading, !bar);
    if let Some(f) = m.fraction {
        let (bx, by, bw) = (x + 14.0, y + HEIGHT - 13.0, WIDTH - 28.0);
        p.fill_round_rect(bx, by, bw, 4.0, 2.0, pal.tertiary.alpha(0.5));
        p.fill_round_rect(bx, by, (bw * f.clamp(0.0, 1.0) as f32).max(4.0), 4.0, 2.0, ACCENT);
    }
}

fn render(hwnd: HWND, s: &State) {
    let (w, h) = (((WIDTH + MARGIN * 2.0) * s.scale).ceil() as i32, ((HEIGHT + MARGIN * 2.0) * s.scale).ceil() as i32);
    let Ok(surface) = Surface::new(w, h) else { return };
    let pal = Palette::current();
    surface.draw(s.scale, |p| paint(p, &s.model, &pal, s.behind.as_ref()));
    surface.present(hwnd, s.origin.0, s.origin.1, 255);
}

/// Liquid Glass: a blurred copy of what is behind the panel, taken as it appears.
fn glass(hwnd: HWND, s: &mut State) {
    s.behind = None;
    if Palette::current().glass {
        let (w, h) = (((WIDTH + MARGIN * 2.0) * s.scale).ceil() as i32, ((HEIGHT + MARGIN * 2.0) * s.scale).ceil() as i32);
        unsafe {
            // Not the panel itself.
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        if let Some(mut img) = super::gfx::capture_screen(s.origin.0, s.origin.1, w, h) {
            super::gfx::blur_rgba(&mut img, w as usize, h as usize, super::theme::blur_sigma() * s.scale);
            s.behind = Some((img, w as u32, h as u32));
        }
    }
}

struct Messages {
    state: Rc<RefCell<State>>,
}

impl Handler for Messages {
    fn message(&self, hwnd: HWND, msg: u32, _wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_NCHITTEST => {
                // Only the × takes clicks; the rest lets them through to what is beneath.
                let s = self.state.try_borrow().ok()?;
                let mut pt = POINT { x: window::loword(lparam.0), y: window::hiword(lparam.0) };
                unsafe {
                    let _ = ScreenToClient(hwnd, &mut pt);
                }
                let (cx, cy) = ((MARGIN + 20.0) * s.scale, (MARGIN + 20.0) * s.scale);
                let d = ((pt.x as f32 - cx).powi(2) + (pt.y as f32 - cy).powi(2)).sqrt();
                Some(LRESULT(if d <= 12.0 * s.scale { HTCLIENT as isize } else { HTTRANSPARENT as isize }))
            }
            WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_LBUTTONDOWN => {
                events::post(UiEvent::HudClose);
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

impl Hud {
    pub fn new() -> Hud {
        let state = Rc::new(RefCell::new(State { model: HudModel::default(), origin: (0, 0), scale: 1.0, visible: false, behind: None }));
        let hwnd = window::create(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            WS_POPUP,
            "Satsuma Progress",
            0,
            0,
            1,
            1,
            None,
            Rc::new(Messages { state: state.clone() }),
        );
        Hud { hwnd, state }
    }

    /// Shows the panel below the wheel position `near` (or above it near the screen bottom).
    pub fn show(&self, near: Point, model: HudModel) {
        let mut s = self.state.borrow_mut();
        s.scale = system::scale_at(near.0, near.1);
        let sc = s.scale;
        let work = system::work_area_at(near.0, near.1);
        let (w, h) = ((WIDTH * sc) as i32, (HEIGHT * sc) as i32);
        let m = (MARGIN * sc) as i32;
        let half_wheel = (150.0 * sc) as i32;
        let mut x = near.0 - w / 2;
        let mut y = near.1 + half_wheel;
        if y + h > work.bottom - (8.0 * sc) as i32 {
            y = near.1 - half_wheel - h;
        }
        let pad = (8.0 * sc) as i32;
        x = x.clamp(work.left + pad, (work.right - w - pad).max(work.left + pad));
        y = y.clamp(work.top + pad, (work.bottom - h - pad).max(work.top + pad));
        s.origin = (x - m, y - m);
        s.model = model;
        s.visible = true;
        glass(self.hwnd, &mut s);
        render(self.hwnd, &s);
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    pub fn update(&self, model: HudModel) {
        let mut s = self.state.borrow_mut();
        s.model = model;
        if s.visible {
            render(self.hwnd, &s);
        }
    }

    pub fn hide(&self) {
        self.state.borrow_mut().visible = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}
