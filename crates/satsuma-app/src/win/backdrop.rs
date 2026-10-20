//! Liquid Glass behind layered windows. Windows blurs a layered window's whole rectangle
//! and ignores its region, so the wheel and the progress panel each get a plain window
//! underneath, shaped by a region, that carries only the blur.

use super::gfx::Surface;
use super::system;
use super::window::{self, Handler};
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::WindowsAndMessaging::*;

struct Messages;

impl Handler for Messages {
    fn message(&self, hwnd: HWND, msg: u32, _w: WPARAM, _l: LPARAM) -> Option<LRESULT> {
        match msg {
            // Fully transparent content: only the blur shows.
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let dc = BeginPaint(hwnd, &mut ps);
                    let mut r = RECT::default();
                    let _ = GetClientRect(hwnd, &mut r);
                    if let Ok(s) = Surface::new(r.right, r.bottom) {
                        s.draw(1.0, |_| {});
                        s.blit(dc, 0, 0);
                    }
                    let _ = EndPaint(hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_NCHITTEST => Some(LRESULT(HTTRANSPARENT as isize)),
            WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
            _ => None,
        }
    }
}

pub struct Backdrop {
    hwnd: HWND,
}

impl Backdrop {
    pub fn new() -> Backdrop {
        let hwnd = window::create(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
            WS_POPUP,
            "Satsuma Glass",
            0,
            0,
            1,
            1,
            None,
            Rc::new(Messages),
        );
        system::set_blur(hwnd, true);
        Backdrop { hwnd }
    }

    /// Blurs the rounded rectangle (or circle, when `radius` is half the size) at `x, y`
    /// (screen pixels), just below `above`.
    pub fn show(&self, above: HWND, x: i32, y: i32, w: i32, h: i32, radius: i32) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER);
        }
        system::set_region(self.hwnd, Some((0, 0, w, h, radius)));
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(above), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            // Keep the content window on top of its glass.
            let _ = SetWindowPos(above, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }
}
