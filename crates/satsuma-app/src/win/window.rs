//! A Win32 window class whose messages go to a Rust handler object.

use std::rc::Rc;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub trait Handler {
    /// Handles a message; `None` falls through to `DefWindowProcW`.
    fn message(&self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT>;
}

const CLASS: &str = "SatsumaWindow";

pub fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default() }
}

fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let class = HSTRING::from(CLASS);
        let icon = LoadIconW(Some(instance()), PCWSTR(1 as _)).unwrap_or_default();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: instance(),
            hIcon: icon,
            hIconSm: icon,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH::default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lparam.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Rc<dyn Handler>;
    let result = if ptr.is_null() {
        None
    } else {
        // Clone so the handler survives a re-entrant WM_NCDESTROY.
        let handler = (*ptr).clone();
        handler.message(hwnd, msg, wparam, lparam)
    };
    if msg == WM_NCDESTROY && !ptr.is_null() {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(Box::from_raw(ptr as *mut Rc<dyn Handler>));
    }
    result.unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
}

#[allow(clippy::too_many_arguments)]
pub fn create(
    ex_style: WINDOW_EX_STYLE,
    style: WINDOW_STYLE,
    title: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    owner: Option<HWND>,
    handler: Rc<dyn Handler>,
) -> HWND {
    register_class();
    let boxed: *mut Rc<dyn Handler> = Box::into_raw(Box::new(handler));
    unsafe {
        CreateWindowExW(
            ex_style,
            &HSTRING::from(CLASS),
            &HSTRING::from(title),
            style,
            x,
            y,
            w,
            h,
            owner,
            None,
            Some(instance()),
            Some(boxed as *const _),
        )
        .unwrap_or_default()
    }
}

pub fn loword(v: isize) -> i32 {
    (v & 0xffff) as u16 as i16 as i32
}

pub fn hiword(v: isize) -> i32 {
    ((v >> 16) & 0xffff) as u16 as i16 as i32
}
