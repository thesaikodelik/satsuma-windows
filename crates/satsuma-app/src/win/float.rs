//! The Float shelf: an always-on-top window holding files. Drag them out into any app,
//! drop more in ("Drop to add"), double-click to open, Delete to remove.

use super::dragsource;
use super::droptarget::{self, DropHandler};
use super::events::{self, Point, UiEvent};
use super::gfx::{Align, Painter, Surface, Weight};
use super::theme::{Palette, ACCENT};
use super::window::{self, Handler};
use super::system;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DROPEFFECT_NONE};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const CELL_W: f32 = 92.0;
const CELL_H: f32 = 96.0;
const PAD: f32 = 10.0;
const ICON: f32 = 52.0;
const MAX_COLS: usize = 4;

struct Item {
    path: PathBuf,
    icon: Option<(Vec<u8>, u32, u32)>,
}

struct State {
    items: Vec<Item>,
    selected: Vec<bool>,
    /// A drag from outside is over the shelf.
    drop_highlight: bool,
    /// The shelf is the source of the current drag (don't accept it back).
    dragging_out: bool,
    press: Option<(i32, i32, usize)>,
}

impl State {
    fn add(&mut self, files: &[PathBuf]) {
        for f in files {
            if !self.items.iter().any(|i| &i.path == f) {
                let icon = system::shell_icon(f, 96);
                self.items.push(Item { path: f.clone(), icon });
                self.selected.push(false);
            }
        }
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.items.iter().zip(&self.selected).filter(|(_, s)| **s).map(|(i, _)| i.path.clone()).collect()
    }

    fn remove_where(&mut self, f: impl Fn(&Item, bool) -> bool) {
        let mut k = 0;
        self.items.retain(|i| {
            let remove = f(i, self.selected[k]);
            k += 1;
            !remove
        });
        self.selected = vec![false; self.items.len()];
    }
}

fn columns(width_dip: f32, n: usize) -> usize {
    (((width_dip - PAD * 2.0) / CELL_W).floor() as usize).clamp(1, n.max(1))
}

fn content_size(n: usize) -> (f32, f32) {
    let cols = n.clamp(1, MAX_COLS);
    let rows = n.max(1).div_ceil(cols);
    (PAD * 2.0 + cols as f32 * CELL_W, PAD * 2.0 + rows as f32 * CELL_H)
}

fn scale_of(hwnd: HWND) -> f32 {
    (unsafe { GetDpiForWindow(hwnd) }.max(96)) as f32 / 96.0
}

fn client_dip(hwnd: HWND) -> (f32, f32) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    let s = scale_of(hwnd);
    ((r.right - r.left) as f32 / s, (r.bottom - r.top) as f32 / s)
}

fn cell_rect(i: usize, cols: usize) -> (f32, f32, f32, f32) {
    let (col, row) = (i % cols, i / cols);
    (PAD + col as f32 * CELL_W, PAD + row as f32 * CELL_H, CELL_W, CELL_H)
}

fn hit(hwnd: HWND, s: &State, x: i32, y: i32) -> Option<usize> {
    let sc = scale_of(hwnd);
    let (w, _) = client_dip(hwnd);
    let cols = columns(w, s.items.len());
    let (x, y) = (x as f32 / sc, y as f32 / sc);
    (0..s.items.len()).find(|&i| {
        let (cx, cy, cw, ch) = cell_rect(i, cols);
        x >= cx && x < cx + cw && y >= cy && y < cy + ch
    })
}

fn paint(p: &Painter, s: &State, size: (f32, f32), pal: &Palette) {
    p.fill_round_rect(0.0, 0.0, size.0, size.1, 0.0, pal.panel.alpha(1.0 / pal.panel.3));
    let cols = columns(size.0, s.items.len());
    if s.items.is_empty() {
        p.text("Drop to add", (0.0, 0.0, size.0, size.1), 12.0, Weight::Semibold, pal.secondary, Align::Center, false);
    }
    for (i, item) in s.items.iter().enumerate() {
        let (x, y, w, _) = cell_rect(i, cols);
        if s.selected[i] {
            p.fill_round_rect(x + 4.0, y + 2.0, w - 8.0, CELL_H - 4.0, 8.0, ACCENT.alpha(0.25));
        }
        let ix = x + (CELL_W - ICON) / 2.0;
        let iy = y + 8.0;
        match &item.icon {
            Some((rgba, iw, ih)) => {
                let s = (ICON / *iw as f32).min(ICON / *ih as f32);
                let (dw, dh) = (*iw as f32 * s, *ih as f32 * s);
                p.image(rgba, *iw, *ih, (ix + (ICON - dw) / 2.0, iy + (ICON - dh) / 2.0, dw, dh));
            }
            None => p.icon(super::theme::glyph("doc.text"), ix + ICON / 2.0, iy + ICON / 2.0, 30.0, pal.secondary),
        }
        let name = item.path.file_name().unwrap_or(item.path.as_os_str()).to_string_lossy();
        p.text(&name, (x + 4.0, y + ICON + 12.0, w - 8.0, 30.0), 11.0, Weight::Regular, pal.label, Align::Center, true);
    }
    if s.drop_highlight {
        p.stroke_round_rect(3.0, 3.0, size.0 - 6.0, size.1 - 6.0, 10.0, ACCENT, 2.0);
    }
}

struct Messages {
    state: Rc<RefCell<State>>,
}

fn refresh(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

/// Resizes the window to fit its items, keeping the top-left corner.
fn fit(hwnd: HWND, n: usize) {
    let s = scale_of(hwnd);
    let (w, h) = content_size(n);
    let mut r = RECT { left: 0, top: 0, right: (w * s) as i32, bottom: (h * s) as i32 };
    unsafe {
        let style = WINDOW_STYLE(GetWindowLongW(hwnd, GWL_STYLE) as u32);
        let ex = WINDOW_EX_STYLE(GetWindowLongW(hwnd, GWL_EXSTYLE) as u32);
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, GetDpiForWindow(hwnd));
        let _ = SetWindowPos(hwnd, None, 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
    refresh(hwnd);
}

impl Handler for Messages {
    fn message(&self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let dc = BeginPaint(hwnd, &mut ps);
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    if let (Ok(surface), Ok(s)) = (Surface::new(r.right, r.bottom), self.state.try_borrow()) {
                        let sc = scale_of(hwnd);
                        let size = (r.right as f32 / sc, r.bottom as f32 / sc);
                        let pal = Palette::current();
                        surface.draw(sc, |p| paint(p, &s, size, &pal));
                        surface.blit(dc, 0, 0);
                    }
                    let _ = EndPaint(hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_LBUTTONDOWN => {
                let (x, y) = (window::loword(lparam.0), window::hiword(lparam.0));
                let mut s = self.state.try_borrow_mut().ok()?;
                let ctrl = (wparam.0 as u32 & 0x0008) != 0; // MK_CONTROL
                match hit(hwnd, &s, x, y) {
                    Some(i) => {
                        if ctrl {
                            s.selected[i] = !s.selected[i];
                        } else if !s.selected[i] {
                            s.selected.iter_mut().for_each(|v| *v = false);
                            s.selected[i] = true;
                        }
                        s.press = Some((x, y, i));
                    }
                    None => {
                        s.selected.iter_mut().for_each(|v| *v = false);
                        s.press = None;
                    }
                }
                refresh(hwnd);
                unsafe { SetCapture(hwnd) };
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = (window::loword(lparam.0), window::hiword(lparam.0));
                let paths = {
                    let s = self.state.try_borrow().ok()?;
                    let Some((px, py, _)) = s.press else { return Some(LRESULT(0)) };
                    let threshold = unsafe { GetSystemMetrics(SM_CXDRAG) };
                    if (x - px).abs() < threshold && (y - py).abs() < threshold {
                        return Some(LRESULT(0));
                    }
                    s.selected_paths()
                };
                {
                    let mut s = self.state.borrow_mut();
                    s.press = None;
                    s.dragging_out = true;
                }
                unsafe {
                    let _ = ReleaseCapture();
                }
                let effect = dragsource::drag_files(&paths).unwrap_or_default();
                let mut s = self.state.borrow_mut();
                s.dragging_out = false;
                // Moved elsewhere (e.g. Shift-drag into a folder): the items are gone.
                if effect.0 & DROPEFFECT_MOVE.0 != 0 {
                    s.remove_where(|i, _| !i.path.exists());
                    let n = s.items.len();
                    drop(s);
                    if n == 0 {
                        unsafe {
                            let _ = DestroyWindow(hwnd);
                        }
                    } else {
                        fit(hwnd, n);
                    }
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.press = None;
                }
                unsafe {
                    let _ = ReleaseCapture();
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDBLCLK => {
                let s = self.state.try_borrow().ok()?;
                if let Some(i) = hit(hwnd, &s, window::loword(lparam.0), window::hiword(lparam.0)) {
                    system::open_item(&s.items[i].path);
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                let mut s = self.state.try_borrow_mut().ok()?;
                if vk == VK_DELETE || vk == VK_BACK {
                    s.remove_where(|_, selected| selected);
                    let n = s.items.len();
                    drop(s);
                    if n == 0 {
                        unsafe {
                            let _ = DestroyWindow(hwnd);
                        }
                    } else {
                        fit(hwnd, n);
                    }
                } else if vk == VK_A && unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0 {
                    s.selected.iter_mut().for_each(|v| *v = true);
                    refresh(hwnd);
                } else if vk == VK_RETURN {
                    for p in s.selected_paths() {
                        system::open_item(&p);
                    }
                }
                Some(LRESULT(0))
            }
            WM_GETMINMAXINFO => {
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                let s = scale_of(hwnd);
                info.ptMinTrackSize.x = ((CELL_W + PAD * 2.0) * s) as i32;
                info.ptMinTrackSize.y = ((CELL_H + PAD * 2.0) * s) as i32;
                Some(LRESULT(0))
            }
            WM_SIZE => {
                refresh(hwnd);
                None
            }
            WM_DESTROY => {
                droptarget::revoke(hwnd);
                events::post(UiEvent::WindowClosed { window: hwnd.0 as isize });
                None
            }
            _ => None,
        }
    }
}

struct Drops {
    hwnd: std::cell::Cell<HWND>,
    state: Rc<RefCell<State>>,
}

impl DropHandler for Drops {
    fn enter(&self, files: Option<Vec<PathBuf>>, _x: i32, _y: i32) -> DROPEFFECT {
        let Ok(mut s) = self.state.try_borrow_mut() else { return DROPEFFECT_NONE };
        if s.dragging_out || files.is_none() {
            return DROPEFFECT_NONE;
        }
        s.drop_highlight = true;
        refresh(self.hwnd.get());
        DROPEFFECT_COPY
    }

    fn over(&self, _x: i32, _y: i32) -> DROPEFFECT {
        match self.state.try_borrow() {
            Ok(s) if s.drop_highlight => DROPEFFECT_COPY,
            _ => DROPEFFECT_NONE,
        }
    }

    fn leave(&self) {
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.drop_highlight = false;
        }
        refresh(self.hwnd.get());
    }

    fn dropped(&self, files: Option<Vec<PathBuf>>, _x: i32, _y: i32) -> DROPEFFECT {
        let Ok(mut s) = self.state.try_borrow_mut() else { return DROPEFFECT_NONE };
        s.drop_highlight = false;
        let (Some(files), false) = (files, s.dragging_out) else {
            refresh(self.hwnd.get());
            return DROPEFFECT_NONE;
        };
        s.add(&files);
        let n = s.items.len();
        drop(s);
        fit(self.hwnd.get(), n);
        // A link, not a copy: the shelf only references the files.
        DROPEFFECT_COPY
    }
}

pub struct FloatWindow {
    hwnd: HWND,
    state: Rc<RefCell<State>>,
}

impl FloatWindow {
    /// A new shelf holding `files`, shown beside the drop point `at`.
    pub fn new(files: &[PathBuf], at: Point) -> FloatWindow {
        let state = Rc::new(RefCell::new(State { items: Vec::new(), selected: Vec::new(), drop_highlight: false, dragging_out: false, press: None }));
        state.borrow_mut().add(files);
        let scale = system::scale_at(at.0, at.1);
        let hwnd = window::create(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            WS_CAPTION | WS_SYSMENU | WS_THICKFRAME,
            "Float",
            at.0 + (40.0 * scale) as i32,
            at.1,
            200,
            200,
            None,
            Rc::new(Messages { state: state.clone() }),
        );
        let drops = Rc::new(Drops { hwnd: std::cell::Cell::new(hwnd), state: state.clone() });
        let _ = droptarget::register(hwnd, drops);
        let n = state.borrow().items.len();
        fit(hwnd, n);
        // Centre vertically on the drop point and keep it on screen.
        unsafe {
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            let work = system::work_area_at(at.0, at.1);
            let x = (at.0 + (40.0 * scale) as i32).clamp(work.left, (work.right - w).max(work.left));
            let y = (at.1 - h / 2).clamp(work.top, (work.bottom - h).max(work.top));
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_SHOWWINDOW);
        }
        FloatWindow { hwnd, state }
    }

    pub fn id(&self) -> isize {
        self.hwnd.0 as isize
    }

    pub fn add(&self, files: &[PathBuf]) {
        self.state.borrow_mut().add(files);
        let n = self.state.borrow().items.len();
        fit(self.hwnd, n);
    }

    pub fn bring_to_front(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNORMAL);
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        }
    }
}
