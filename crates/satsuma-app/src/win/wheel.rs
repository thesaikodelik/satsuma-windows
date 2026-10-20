//! The radial wheel: a topmost, non-activating layered window drawn with Direct2D and
//! registered as an OLE drop target.
//!
//! Windows has no shared drag pasteboard to watch, so the wheel window doubles as the drag
//! probe: when a wheel key is tapped while the mouse button is held, the window appears
//! under the pointer at alpha 1/255 (invisible, but still hit-testable). If the drag loop
//! delivers a drag carrying files to it, the wheel turns visible; otherwise it disappears.

use super::droptarget::{self, DropHandler};
use super::liquid::{bounds, sector_sdf, Liquid, Sector};
use super::events::{self, Point, UiEvent};
use super::gfx::{Align, Painter, Rgba, Surface, Weight};
use super::theme::{glyph, Palette, ACCENT};
use super::window::{self, Handler};
use super::system;
use satsuma_core::geometry::{WheelGeometry, WHEEL_VIEW_SIZE};
use satsuma_core::wheel::{empty_message, navigate, NavKey};
use satsuma_core::{WheelCombos, WheelItem, WheelMode};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Debug)]
pub struct WheelModel {
    pub mode: WheelMode,
    pub items: Vec<WheelItem>,
    pub selected: Option<usize>,
    /// Opened from the menu (keyboard and clicks) rather than during a drag.
    pub keyboard: bool,
    pub feedback: bool,
    pub combos: WheelCombos,
    /// Centre text when nothing is highlighted, e.g. ["1.2 MB"].
    pub hub: Vec<String>,
    /// Output size per petal ("1.4 MB", "≈ 12 MB"); `None` when it can't be previewed.
    pub estimates: HashMap<WheelItem, Option<String>>,
    /// Sizes are being worked out, so a petal without one shows bouncing dots.
    pub estimating: bool,
    /// Seconds into the dots' animation.
    pub phase: f32,
    /// The liquid highlight (physics), chasing the selected petal.
    pub liquid: Liquid,
    /// Drawn on a window that is glass already (the Settings preview).
    pub on_glass_window: bool,
}

impl WheelModel {
    pub fn new(mode: WheelMode, items: Vec<WheelItem>) -> WheelModel {
        WheelModel {
            mode,
            items,
            selected: None,
            keyboard: false,
            feedback: false,
            combos: WheelCombos::default(),
            hub: Vec::new(),
            estimates: HashMap::new(),
            estimating: false,
            phase: 0.0,
            liquid: Liquid::default(),
            on_glass_window: false,
        }
    }
}

struct State {
    model: WheelModel,
    /// Centre of the wheel, screen pixels.
    anchor: Point,
    /// Pixels per DIP on the wheel's monitor.
    scale: f32,
    visible: bool,
    probing: bool,
    /// Files of the drag currently over the wheel.
    files: Vec<PathBuf>,
    palette: Palette,
    behind: Option<Behind>,
    started: std::time::Instant,
}

pub struct WheelWindow {
    pub hwnd: HWND,
    state: Rc<RefCell<State>>,
}

fn size_px(scale: f32) -> i32 {
    (WHEEL_VIEW_SIZE as f32 * scale).ceil() as i32
}

/// The ring sector a petal occupies (what the liquid chases).
pub fn petal_sector(i: usize, n: usize) -> Sector {
    let g = WheelGeometry::default();
    let mid = ((g.inner + g.outer) / 2.0) as f32;
    let thick = ((g.outer - g.inner) / 2.0) as f32;
    let half = if n <= 1 { std::f32::consts::PI } else { std::f32::consts::PI / n as f32 - (g.gap as f32 / 2.0) / mid };
    Sector { angle: WheelGeometry::petal_angle(i, n) as f32, half, mid, thick }
}

impl WheelModel {
    /// Points the liquid at the selected petal (or lets it fade).
    pub fn aim_liquid(&mut self) {
        let n = self.items.len();
        self.liquid.set_target(self.selected.filter(|i| *i < n).map(|i| petal_sector(i, n)));
    }

    /// Puts the liquid at rest on the selected petal, without motion.
    pub fn settle_liquid(&mut self) {
        let n = self.items.len();
        self.liquid = Liquid::default();
        if let Some(i) = self.selected.filter(|i| *i < n) {
            self.liquid.settle_on(petal_sector(i, n));
        }
    }
}

/// A blurred copy of what is behind the wheel (RGBA, width, height), for Liquid Glass.
pub type Behind = (Vec<u8>, u32, u32);

/// 30nama's glare: bright over the top half, a little caught along the bottom (`v` 0–1, top to bottom).
fn nama_glare(lit: bool, v: f32) -> f32 {
    let (top, mid, bottom) = if lit { (0.30, 0.06, 0.10) } else { (0.12, 0.02, 0.04) };
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t.clamp(0.0, 1.0);
    if v < 0.5 {
        lerp(top, mid, v / 0.5)
    } else if v < 0.7 {
        lerp(mid, 0.0, (v - 0.5) / 0.2)
    } else {
        lerp(0.0, bottom, (v - 0.7) / 0.3)
    }
}

/// 30nama's rim light: bright where light strikes the top edge, faint along the bottom.
fn nama_rim(lit: bool, v: f32) -> f32 {
    let stops = if lit { [0.95, 0.25, 0.05, 0.45] } else { [0.55, 0.12, 0.05, 0.22] };
    let t = (v.clamp(0.0, 1.0) * 3.0).min(2.999);
    let k = t.floor() as usize;
    stops[k] + (stops[k + 1] - stops[k]) * (t - k as f32)
}

fn white(a: f32) -> Rgba {
    Rgba(1.0, 1.0, 1.0, a)
}

/// The vertical extent (y down) of a sector drawn around `c`.
fn sector_rows(s: Sector, c: f32) -> (f32, f32) {
    let (mut top, mut bottom) = (f32::MAX, f32::MIN);
    for k in 0..=8 {
        let a = s.angle - s.half + s.half * 2.0 * k as f32 / 8.0;
        for r in [s.mid - s.thick, s.mid + s.thick] {
            let y = c - r * a.sin();
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    (top, bottom)
}

/// Draws a wheel model centred in a `WHEEL_VIEW_SIZE` square (DIPs); `scale` is pixels per DIP.
pub fn paint(p: &Painter, m: &WheelModel, pal: &Palette, behind: Option<&Behind>, scale: f32) {
    let g = WheelGeometry::default();
    let c = (WHEEL_VIEW_SIZE / 2.0) as f32;
    let backdrop = g.backdrop as f32;
    let disc = (c - backdrop, c - backdrop, backdrop * 2.0, backdrop * 2.0);
    if pal.glass {
        // A soft shadow under the glass (black 22 %, radius 7, 3 down).
        for k in (1..=5).rev() {
            p.fill_circle(c, c + 3.0, backdrop + k as f32 * 1.3, Rgba(0.0, 0.0, 0.0, 0.22 / 6.0));
        }
    }
    if let (true, Some((px, w, h))) = (pal.glass, behind) {
        let size = WHEEL_VIEW_SIZE as f32;
        p.fill_round_rect_image(disc, backdrop, (0.0, 0.0, size, size), px, *w, *h);
    }
    // On a glass window (the Settings preview) the window's tint is the disc's tint already.
    if !(pal.glass && m.on_glass_window) {
        p.fill_circle(c, c, backdrop, pal.backdrop);
    }
    if pal.glass {
        // 30nama's drawn glass: a faint body, a glare from above and a lit rim.
        p.fill_circle(c, c, backdrop, white(0.02));
        let glare: Vec<(f32, Rgba)> = [0.0, 0.5, 0.7, 1.0].iter().map(|v| (*v, white(nama_glare(false, *v)))).collect();
        p.round_rect_gradient(disc.0, disc.1, disc.2, disc.3, backdrop, &glare, (c, disc.1), (c, disc.1 + disc.3), None);
        let rim: Vec<(f32, Rgba)> = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0].iter().map(|v| (*v, white(nama_rim(false, *v)))).collect();
        p.round_rect_gradient(disc.0 + 0.6, disc.1 + 0.6, disc.2 - 1.2, disc.3 - 1.2, backdrop - 0.6, &rim, (c, disc.1), (c, disc.1 + disc.3), Some(1.2));
    } else {
        p.stroke_circle(c, c, backdrop - 0.25, pal.separator, 0.5);
    }

    if m.items.is_empty() {
        p.text(empty_message(m.mode), (c - 85.0, c - 30.0, 170.0, 60.0), 12.0, Weight::Semibold, pal.secondary, Align::Center, true);
        return;
    }
    let n = m.items.len();
    let dense = n > 6;
    // Plain petals; the highlight is drawn on top as liquid so it can flow between them.
    for i in 0..n {
        if let Some(geo) = p.path(&g.petal_path(i, n), c, c) {
            if pal.glass {
                // 30nama's tabs have no surface of their own; a whisper keeps petals apart.
                p.fill_geometry(&geo, white(0.035));
            } else {
                p.fill_geometry(&geo, pal.control.alpha(0.92));
                p.stroke_geometry(&geo, pal.separator, 0.5);
            }
        }
    }
    let pieces = m.liquid.pieces();
    let liquid = |x: f32, y: f32| pieces.iter().map(|s| sector_sdf(x - c, c - y, *s)).reduce(super::gfx::smooth_union).unwrap_or(f32::MAX);
    // Only the box around the liquid is shaded, which keeps every frame cheap.
    if let Some(area) = bounds(&pieces, c) {
        if pal.glass {
            // The lit glass pill: white body and glare over the pill's height, a bright rim.
            let (top, bottom) = pieces.iter().map(|s| sector_rows(*s, c)).fold((f32::MAX, f32::MIN), |a, b| (a.0.min(b.0), a.1.max(b.1)));
            let span = (bottom - top).max(1.0);
            p.sdf_fill_lit(area, scale, &|y| {
                let v = ((y - top) / span).clamp(0.0, 1.0);
                (white(0.10 + nama_glare(true, v)), white(nama_rim(true, v)))
            }, &liquid);
        } else {
            p.sdf_fill(area, scale, ACCENT, white(0.0), &liquid);
        }
    }
    for (i, item) in m.items.iter().enumerate() {
        let lp = g.label_position(i, n);
        let (x, y) = (c + lp.x as f32, c - lp.y as f32);
        // A label is lit while the liquid is under it.
        let lit = !pieces.is_empty() && liquid(x, y) < -2.0;
        let color = match (pal.glass, lit) {
            (true, true) => pal.label,
            (true, false) => pal.secondary,
            (false, true) => Rgba::WHITE,
            (false, false) => pal.label,
        };
        let title = item.title();
        match item.symbol() {
            Some(name) => {
                p.icon(glyph(name), x, y - 8.0, if dense { 13.0 } else { 15.0 }, color);
                p.text_centered(&title, x, y + 10.0, if dense { 7.5 } else { 8.5 }, Weight::Bold, color);
            }
            None => p.text_centered(&title, x, y, if dense { 10.5 } else { 12.0 }, Weight::Bold, color),
        }
    }

    let selected = m.selected.and_then(|i| m.items.get(i));
    let accent = if pal.glass { pal.label } else { ACCENT };
    if selected.is_some() || !m.hub.is_empty() {
        let r = (g.inner - 6.0) as f32;
        if pal.glass {
            p.fill_circle(c, c, r, white(0.04));
            let rim: Vec<(f32, Rgba)> = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0].iter().map(|v| (*v, white(nama_rim(false, *v)))).collect();
            p.round_rect_gradient(c - r + 0.6, c - r + 0.6, 2.0 * r - 1.2, 2.0 * r - 1.2, r - 0.6, &rim, (c, c - r), (c, c + r), Some(1.2));
        } else {
            p.fill_circle(c, c, r, pal.control.alpha(if selected.is_some() { 1.0 } else { 0.85 }));
        }
    }
    if let Some(item) = selected {
        // The result's size, with the petal's name under it.
        let size = match m.estimates.get(item) {
            Some(Some(text)) => Some(text.as_str()),
            _ => None,
        };
        if size.is_none() && m.estimating && !m.estimates.contains_key(item) {
            // Working it out: three dots hop one after another.
            for k in 0..3 {
                let t = (m.phase * 2.4 - k as f32 * 0.18).rem_euclid(1.0);
                let hop = if t < 0.5 { (t * 2.0 * std::f32::consts::PI).sin().max(0.0) } else { 0.0 };
                p.fill_circle(c - 8.0 + k as f32 * 8.0, c - 4.0 - hop * 4.5, 2.4, accent.alpha(0.55 + 0.45 * hop));
            }
            p.text_centered(&item.title(), c, c + 10.0, 7.5, Weight::Semibold, pal.secondary);
            return;
        }
        match size {
            Some(text) => {
                p.text_centered(text, c, c - 5.0, if text.chars().count() > 8 { 10.0 } else { 11.5 }, Weight::Bold, accent);
                p.text_centered(&item.title(), c, c + 10.0, 7.5, Weight::Semibold, pal.secondary);
            }
            None => p.text_centered(&item.title(), c, c, 9.5, Weight::Bold, accent),
        }
    } else if !m.hub.is_empty() {
        let n = m.hub.len() as f32;
        for (k, line) in m.hub.iter().enumerate() {
            let (size, color) = if k == 0 { (10.5, pal.label) } else { (9.0, pal.secondary) };
            let y = c - (n - 1.0) * 6.5 + k as f32 * 13.0;
            p.text_centered(line, c, y, size, Weight::Semibold, color);
        }
    }
}

impl State {
    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let dx = (x - self.anchor.0) as f64 / self.scale as f64;
        let dy = -((y - self.anchor.1) as f64) / self.scale as f64;
        WheelGeometry::default().hit_test(dx, dy, self.model.items.len())
    }

    fn origin(&self) -> Point {
        let half = size_px(self.scale) / 2;
        (self.anchor.0 - half, self.anchor.1 - half)
    }
}

/// Liquid Glass: a blurred copy of what is behind the wheel, taken as it appears (the
/// system blur can't follow a round window, so the wheel blurs it itself).
fn glass(s: &mut State) {
    s.behind = None;
    if s.palette.glass && !s.probing && s.visible {
        let px = size_px(s.scale);
        let (x, y) = s.origin();
        if let Some(mut img) = super::gfx::capture_screen(x, y, px, px) {
            super::gfx::blur_rgba(&mut img, px as usize, px as usize, super::theme::blur_sigma() * s.scale);
            s.behind = Some((img, px as u32, px as u32));
        }
    }
}

fn render(hwnd: HWND, s: &State) {
    let px = size_px(s.scale);
    Surface::with(px, px, |surface| {
        if s.probing {
            surface.draw(1.0, |_| {});
            surface.fill_transparent(1);
        } else {
            surface.draw(s.scale, |p| paint(p, &s.model, &s.palette, s.behind.as_ref(), s.scale));
        }
        let (x, y) = s.origin();
        surface.present(hwnd, x, y, 255);
    });
}

/// Changes the highlighted petal; plays the tick when enabled.
fn set_selected(hwnd: HWND, s: &mut State, sel: Option<usize>) {
    if s.model.selected == sel {
        return;
    }
    s.model.selected = sel;
    s.model.aim_liquid();
    if sel.is_some() && s.model.feedback {
        system::highlight_feedback();
    }
    if s.visible && !s.probing {
        render(hwnd, s);
    }
}

struct Drops {
    hwnd: std::cell::Cell<HWND>,
    state: Rc<RefCell<State>>,
}

impl Drops {
    fn effect(&self, x: i32, y: i32) -> DROPEFFECT {
        let Ok(mut s) = self.state.try_borrow_mut() else { return DROPEFFECT_NONE };
        if s.probing || s.files.is_empty() {
            return DROPEFFECT_NONE;
        }
        let hit = s.hit(x, y);
        set_selected(self.hwnd.get(), &mut s, hit);
        if hit.is_some() {
            DROPEFFECT_COPY
        } else {
            DROPEFFECT_NONE
        }
    }
}

impl DropHandler for Drops {
    fn enter(&self, files: Option<Vec<PathBuf>>, x: i32, y: i32) -> DROPEFFECT {
        debug!("wheel DragEnter at ({x}, {y}) with {:?}", files.as_ref().map(|f| f.len()));
        {
            let Ok(mut s) = self.state.try_borrow_mut() else { return DROPEFFECT_NONE };
            match files {
                Some(f) => {
                    if s.probing {
                        events::post(UiEvent::ProbeConfirmed { files: f.clone() });
                    }
                    s.files = f;
                }
                None => {
                    s.files.clear();
                    if s.probing {
                        events::post(UiEvent::ProbeRejected);
                    }
                    return DROPEFFECT_NONE;
                }
            }
        }
        self.effect(x, y)
    }

    fn over(&self, x: i32, y: i32) -> DROPEFFECT {
        self.effect(x, y)
    }

    fn leave(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            set_selected(self.hwnd.get(), &mut s, None);
        }
    }

    fn dropped(&self, files: Option<Vec<PathBuf>>, x: i32, y: i32) -> DROPEFFECT {
        let Ok(s) = self.state.try_borrow() else { return DROPEFFECT_NONE };
        let files = files.unwrap_or_else(|| s.files.clone());
        if s.probing || files.is_empty() {
            return DROPEFFECT_NONE;
        }
        match s.hit(x, y).and_then(|i| s.model.items.get(i)) {
            Some(item) => {
                events::post(UiEvent::WheelDrop { item: *item, files, at: s.anchor });
                DROPEFFECT_COPY
            }
            // Off a petal: refuse, so the drag is cancelled.
            None => DROPEFFECT_NONE,
        }
    }
}

struct Messages {
    state: Rc<RefCell<State>>,
}

impl Handler for Messages {
    fn message(&self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEACTIVATE => {
                // Drag mode never takes focus; the keyboard wheel already has it.
                let keyboard = self.state.try_borrow().map(|s| s.model.keyboard).unwrap_or(false);
                Some(LRESULT(if keyboard { MA_ACTIVATE } else { MA_NOACTIVATE } as isize))
            }
            WM_MOUSEMOVE => {
                let mut s = self.state.try_borrow_mut().ok()?;
                if s.model.keyboard {
                    let (ox, oy) = s.origin();
                    let hit = s.hit(ox + window::loword(lparam.0), oy + window::hiword(lparam.0));
                    if hit.is_some() {
                        set_selected(hwnd, &mut s, hit);
                    }
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                let s = self.state.try_borrow().ok()?;
                if s.model.keyboard {
                    let (ox, oy) = s.origin();
                    match s.hit(ox + window::loword(lparam.0), oy + window::hiword(lparam.0)).and_then(|i| s.model.items.get(i)) {
                        Some(item) => events::post(UiEvent::WheelPick { item: *item }),
                        None => events::post(UiEvent::WheelCancel),
                    }
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let mut s = self.state.try_borrow_mut().ok()?;
                if !s.model.keyboard {
                    return Some(LRESULT(0));
                }
                let n = s.model.items.len();
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                let nav = match vk {
                    VK_RIGHT | VK_DOWN => Some(NavKey::Next),
                    VK_LEFT | VK_UP => Some(NavKey::Previous),
                    VK_HOME => Some(NavKey::First),
                    VK_END => Some(NavKey::Last),
                    _ => None,
                };
                if let Some(key) = nav {
                    let sel = navigate(s.model.selected, n, key);
                    set_selected(hwnd, &mut s, sel);
                } else if vk == VK_RETURN || vk == VK_SPACE {
                    if let Some(item) = s.model.selected.and_then(|i| s.model.items.get(i)) {
                        events::post(UiEvent::WheelPick { item: *item });
                    }
                } else if vk == VK_ESCAPE || vk == VK_TAB {
                    events::post(UiEvent::WheelCancel);
                } else if matches!(vk, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LSHIFT | VK_RSHIFT | VK_LCONTROL | VK_RCONTROL | VK_LMENU | VK_RMENU) {
                    if let Some(mode) = s.model.combos.mode_for(system::current_mods()) {
                        events::post(UiEvent::WheelSwitch(mode));
                    }
                }
                Some(LRESULT(0))
            }
            // Keep Alt from opening a (nonexistent) window menu.
            WM_SYSKEYUP | WM_SYSCHAR => Some(LRESULT(0)),
            WM_ACTIVATE => {
                if (wparam.0 & 0xffff) as u32 == WA_INACTIVE {
                    events::post(UiEvent::WindowDeactivated { window: hwnd.0 as isize });
                }
                None
            }
            _ => None,
        }
    }
}

impl WheelWindow {
    pub fn new() -> WheelWindow {
        let state = Rc::new(RefCell::new(State {
            model: WheelModel::new(WheelMode::Conversions, Vec::new()),
            anchor: (0, 0),
            scale: 1.0,
            visible: false,
            probing: false,
            files: Vec::new(),
            palette: Palette::current(),
            behind: None,
            started: std::time::Instant::now(),
        }));
        let hwnd = window::create(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            WS_POPUP,
            "Satsuma Wheel",
            0,
            0,
            size_px(1.0),
            size_px(1.0),
            None,
            Rc::new(Messages { state: state.clone() }),
        );
        let drops = Rc::new(Drops { hwnd: std::cell::Cell::new(hwnd), state: state.clone() });
        if let Err(e) = droptarget::register(hwnd, drops) {
            debug!("RegisterDragDrop failed: {e}");
        }
        WheelWindow { hwnd, state }
    }

    fn place(&self, at: Point, keyboard: bool) {
        let mut s = self.state.borrow_mut();
        s.scale = system::scale_at(at.0, at.1);
        s.palette = Palette::current();
        let work = system::work_area_at(at.0, at.1);
        let sc = s.scale as f64;
        // Clamp in DIPs so the whole wheel stays on the monitor.
        let anchor = WheelGeometry::default().clamp_anchor(
            satsuma_core::geometry::Point::new(at.0 as f64 / sc, at.1 as f64 / sc),
            (work.left as f64 / sc, work.top as f64 / sc, (work.right - work.left) as f64 / sc, (work.bottom - work.top) as f64 / sc),
        );
        s.anchor = ((anchor.x * sc).round() as i32, (anchor.y * sc).round() as i32);
        s.visible = true;
        glass(&mut s);
        render(self.hwnd, &s);
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            if keyboard {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
        }
    }

    /// Shows an invisible probe under the pointer to find out whether a file drag is running.
    pub fn probe(&self, at: Point) {
        {
            let mut s = self.state.borrow_mut();
            s.probing = true;
            s.palette = Palette::current();
            s.files.clear();
            s.model = WheelModel::new(WheelMode::Conversions, Vec::new());
            s.anchor = at;
        }
        // The probe goes exactly under the pointer (no clamping): it must be hit.
        let mut s = self.state.borrow_mut();
        s.scale = system::scale_at(at.0, at.1);
        s.visible = true;
        glass(&mut s);
        render(self.hwnd, &s);
        drop(s);
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            let under = WindowFromPoint(windows::Win32::Foundation::POINT { x: at.0, y: at.1 });
            if under == self.hwnd {
                debug!("probe shown under the pointer");
            } else {
                let mut class = [0u16; 64];
                let mut title = [0u16; 64];
                let n = GetClassNameW(under, &mut class) as usize;
                let t = GetWindowTextW(under, &mut title) as usize;
                debug!(
                    "probe shown, but the window under the pointer is {:?} \"{}\"",
                    String::from_utf16_lossy(&class[..n]),
                    String::from_utf16_lossy(&title[..t])
                );
            }
        }
    }

    /// The probe found files: show the real wheel in its place.
    pub fn confirm(&self, model: WheelModel) {
        let at = {
            let mut s = self.state.borrow_mut();
            s.probing = false;
            s.model = model;
            s.model.settle_liquid();
            s.anchor
        };
        self.place(at, false);
    }

    pub fn show(&self, model: WheelModel, at: Point) {
        let keyboard = model.keyboard;
        {
            let mut s = self.state.borrow_mut();
            s.probing = false;
            s.model = model;
            s.model.settle_liquid();
        }
        self.place(at, keyboard);
    }

    pub fn switch(&self, mode: WheelMode, items: Vec<WheelItem>) {
        let mut s = self.state.borrow_mut();
        s.model.selected = if s.model.keyboard && !items.is_empty() { Some(0) } else { None };
        s.model.mode = mode;
        s.model.items = items;
        s.model.settle_liquid();
        if s.visible && !s.probing {
            render(self.hwnd, &s);
        }
    }

    pub fn hide(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.visible = false;
            s.probing = false;
            s.model.keyboard = false;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Records a petal's output size (or that it has none) and redraws if it is showing.
    pub fn set_estimate(&self, item: WheelItem, text: Option<String>) {
        let mut s = self.state.borrow_mut();
        s.model.estimates.insert(item, text);
        if s.visible && !s.probing && s.model.selected.and_then(|i| s.model.items.get(i)) == Some(&item) {
            render(self.hwnd, &s);
        }
    }

    pub fn set_estimating(&self, on: bool) {
        let mut s = self.state.borrow_mut();
        s.model.estimating = on;
        s.model.estimates.clear();
    }

    /// Advances the waiting dots while the hovered petal's size is being worked out.
    pub fn animate(&self) {
        let Ok(mut s) = self.state.try_borrow_mut() else { return };
        let waiting = s.visible
            && !s.probing
            && s.model.estimating
            && s.model.selected.and_then(|i| s.model.items.get(i)).is_some_and(|item| !s.model.estimates.contains_key(item));
        let flowing = s.visible && !s.probing && s.model.liquid.moving();
        if flowing {
            s.model.liquid.step(std::time::Instant::now());
        }
        if waiting || flowing {
            s.model.phase = s.started.elapsed().as_secs_f32();
            render(self.hwnd, &s);
        }
    }

    pub fn selected_item(&self) -> Option<WheelItem> {
        let s = self.state.try_borrow().ok()?;
        s.model.selected.and_then(|i| s.model.items.get(i).copied())
    }

    /// Shown at all, probe included.
    pub fn is_visible(&self) -> bool {
        self.state.try_borrow().map(|s| s.visible).unwrap_or(true)
    }

    pub fn is_open(&self) -> bool {
        self.state.try_borrow().map(|s| s.visible && !s.probing).unwrap_or(true)
    }

    pub fn anchor(&self) -> Point {
        self.state.borrow().anchor
    }

    pub fn has_selection(&self) -> bool {
        self.state.borrow().model.selected.is_some()
    }
}

/// Renders a wheel to a PNG at 2× (for `satsuma render-wheel`).
pub fn render_png(model: &WheelModel, dark: bool, out: &std::path::Path) -> Result<(), String> {
    let scale = 2.0;
    let px = size_px(scale);
    let surface = Surface::new(px, px).map_err(|e| e.message())?;
    let pal = Palette::for_appearance(super::render_theme(), dark);
    surface.draw(scale, |p| paint(p, model, &pal, None, scale));
    super::gfx::save_png(&surface.rgba(), px as u32, px as u32, out)
}
