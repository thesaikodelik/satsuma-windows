//! The radial wheel: a topmost, non-activating layered window drawn with Direct2D and
//! registered as an OLE drop target.
//!
//! Windows has no shared drag pasteboard to watch, so the wheel window doubles as the drag
//! probe: when a wheel key is tapped while the mouse button is held, the window appears
//! under the pointer at alpha 1/255 (invisible, but still hit-testable). If the drag loop
//! delivers a drag carrying files to it, the wheel turns visible; otherwise it disappears.

use super::droptarget::{self, DropHandler};
use super::events::{self, Point, UiEvent};
use super::gfx::{Align, Painter, Rgba, Surface, Weight};
use super::theme::{glyph, Palette, ACCENT};
use super::window::{self, Handler};
use super::system;
use satsuma_core::geometry::{WheelGeometry, WHEEL_VIEW_SIZE};
use satsuma_core::wheel::{empty_message, navigate, NavKey};
use satsuma_core::{WheelCombos, WheelItem, WheelMode};
use std::cell::RefCell;
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
}

impl WheelModel {
    pub fn new(mode: WheelMode, items: Vec<WheelItem>) -> WheelModel {
        WheelModel { mode, items, selected: None, keyboard: false, feedback: false, combos: WheelCombos::default(), hub: Vec::new() }
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
}

pub struct WheelWindow {
    pub hwnd: HWND,
    state: Rc<RefCell<State>>,
}

fn size_px(scale: f32) -> i32 {
    (WHEEL_VIEW_SIZE as f32 * scale).ceil() as i32
}

/// Draws a wheel model centred in a `WHEEL_VIEW_SIZE` square (DIPs).
pub fn paint(p: &Painter, m: &WheelModel, pal: &Palette) {
    let g = WheelGeometry::default();
    let c = (WHEEL_VIEW_SIZE / 2.0) as f32;
    let backdrop = g.backdrop as f32;
    p.fill_circle(c, c, backdrop, pal.backdrop);
    p.stroke_circle(c, c, backdrop - 0.25, pal.separator, 0.5);

    if m.items.is_empty() {
        p.text(empty_message(m.mode), (c - 85.0, c - 30.0, 170.0, 60.0), 12.0, Weight::Semibold, pal.secondary, Align::Center, true);
        return;
    }
    let n = m.items.len();
    let dense = n > 6;
    for (i, item) in m.items.iter().enumerate() {
        let highlighted = m.selected == Some(i);
        if let Some(geo) = p.path(&g.petal_path(i, n), c, c) {
            p.fill_geometry(&geo, if highlighted { ACCENT } else { pal.control.alpha(0.92) });
            if !highlighted {
                p.stroke_geometry(&geo, pal.separator, 0.5);
            }
        }
        let color = if highlighted { Rgba::WHITE } else { pal.label };
        let lp = g.label_position(i, n);
        let (x, y) = (c + lp.x as f32, c - lp.y as f32);
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
    if selected.is_some() || !m.hub.is_empty() {
        let r = (g.inner - 6.0) as f32;
        p.fill_circle(c, c, r, pal.control.alpha(if selected.is_some() { 1.0 } else { 0.85 }));
    }
    if let Some(item) = selected {
        p.text_centered(&item.title(), c, c, 9.5, Weight::Bold, ACCENT);
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

fn render(hwnd: HWND, s: &State) {
    let px = size_px(s.scale);
    let Ok(surface) = Surface::new(px, px) else { return };
    if s.probing {
        surface.draw(1.0, |_| {});
        surface.fill_transparent(1);
    } else {
        surface.draw(s.scale, |p| paint(p, &s.model, &s.palette));
    }
    let (x, y) = s.origin();
    surface.present(hwnd, x, y, 255);
}

/// Changes the highlighted petal; plays the tick when enabled.
fn set_selected(hwnd: HWND, s: &mut State, sel: Option<usize>) {
    if s.model.selected == sel {
        return;
    }
    s.model.selected = sel;
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
            s.files.clear();
            s.model = WheelModel::new(WheelMode::Conversions, Vec::new());
            s.anchor = at;
        }
        // The probe goes exactly under the pointer (no clamping): it must be hit.
        let mut s = self.state.borrow_mut();
        s.scale = system::scale_at(at.0, at.1);
        s.visible = true;
        render(self.hwnd, &s);
        drop(s);
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            let under = WindowFromPoint(windows::Win32::Foundation::POINT { x: at.0, y: at.1 });
            debug!("probe shown; window under pointer is the probe: {}", under == self.hwnd);
        }
    }

    /// The probe found files: show the real wheel in its place.
    pub fn confirm(&self, model: WheelModel) {
        let at = {
            let mut s = self.state.borrow_mut();
            s.probing = false;
            s.model = model;
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
        }
        self.place(at, keyboard);
    }

    pub fn switch(&self, mode: WheelMode, items: Vec<WheelItem>) {
        let mut s = self.state.borrow_mut();
        s.model.selected = if s.model.keyboard && !items.is_empty() { Some(0) } else { None };
        s.model.mode = mode;
        s.model.items = items;
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
    let pal = Palette::new(dark);
    surface.draw(scale, |p| paint(p, model, &pal));
    super::gfx::save_png(&surface.rgba(), px as u32, px as u32, out)
}
