//! Debug builds: `satsuma-cli selftest-drag [--key shift|ctrl|alt] [--petal N] [--center] <files…>`
//! drives a real OLE drag from a small window of its own, taps a wheel key mid-drag and
//! drops on petal N of the running app's wheel, using synthesized input (`SendInput`).
//! `--center` drops in the hub instead (which must cancel).

use super::dragsource;
use super::window::{self, Handler};
use satsuma_core::geometry::WheelGeometry;
use satsuma_core::settings::Settings;
use satsuma_core::wheel::wheel_items;
use satsuma_core::Mods;
use satsuma_engine::{capability, detect_format, Env};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn mouse(flags: MOUSE_EVENT_FLAGS) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn glide(from: (i32, i32), to: (i32, i32), steps: i32) {
    for k in 1..=steps {
        let x = from.0 + (to.0 - from.0) * k / steps;
        let y = from.1 + (to.1 - from.1) * k / steps;
        unsafe {
            let _ = SetCursorPos(x, y);
        }
        std::thread::sleep(Duration::from_millis(12));
    }
}

/// Captures a screen rectangle (layered windows included) to a PNG.
fn screenshot(x: i32, y: i32, w: i32, h: i32, out: &std::path::Path) -> Result<(), String> {
    use windows::Win32::Graphics::Gdi::*;
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let _ = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        GetDIBits(mem, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        let rgba: Vec<u8> = buf.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        super::gfx::save_png(&rgba, w as u32, h as u32, out)
    }
}

struct Source {
    files: Vec<PathBuf>,
    result: Rc<RefCell<Option<String>>>,
}

impl Handler for Source {
    fn message(&self, _hwnd: HWND, msg: u32, _w: WPARAM, _l: LPARAM) -> Option<LRESULT> {
        if msg == WM_LBUTTONDOWN {
            let r = dragsource::drag_files(&self.files);
            *self.result.borrow_mut() = Some(match r {
                Ok(e) => format!("drop effect {}", e.0),
                Err(e) => format!("drag failed: {e}"),
            });
            unsafe { PostQuitMessage(0) };
            return Some(LRESULT(0));
        }
        None
    }
}

pub fn selftest_drag(args: &[String]) -> i32 {
    let mut vk = VK_SHIFT;
    let mut mods = Mods::SHIFT;
    let mut petal = 0usize;
    let mut center = false;
    let mut shot: Option<PathBuf> = None;
    let mut from: Option<(i32, i32)> = None;
    let mut hold = false;
    let mut also: Option<VIRTUAL_KEY> = None;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--key" => match it.next().map(String::as_str) {
                Some("ctrl") => (vk, mods) = (VK_CONTROL, Mods::CONTROL),
                Some("alt") => (vk, mods) = (VK_MENU, Mods::OPTION),
                _ => (vk, mods) = (VK_SHIFT, Mods::SHIFT),
            },
            "--petal" => petal = it.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            "--center" => center = true,
            "--screenshot" => shot = it.next().map(PathBuf::from),
            // Start the drag from another app (e.g. a file icon in File Explorer) at X,Y.
            "--from" => from = it.next().and_then(|v| v.split_once(',')).and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?))),
            // Keep the key held until the drop instead of tapping it.
            "--hold" => hold = true,
            // Hold another key during the drag first (Shift to move, Ctrl to copy …).
            "--also" => {
                also = match it.next().map(String::as_str) {
                    Some("ctrl") => Some(VK_CONTROL),
                    Some("alt") => Some(VK_MENU),
                    _ => Some(VK_SHIFT),
                }
            }
            f => files.push(dunce::simplified(&std::fs::canonicalize(f).unwrap_or_else(|_| PathBuf::from(f))).to_path_buf()),
        }
    }
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
    }
    if files.is_empty() {
        eprintln!("Usage: satsuma-cli selftest-drag [--key shift|ctrl|alt] [--petal N] [--center] <files…>");
        return 2;
    }
    // Where petal N of the wheel this key opens will be.
    let settings = Settings::load(&Settings::default_path());
    let Some(mode) = settings.wheels.mode_for(mods) else {
        eprintln!("{} opens no wheel with the current settings.", mods.label());
        return 2;
    };
    let env = Env::discover();
    let formats: Vec<_> = files.iter().map(|p| detect_format(p)).collect();
    let items = wheel_items(mode, &formats, &|i| capability::item_available(&env, i, &formats));
    if items.is_empty() || petal >= items.len() {
        eprintln!("The {} wheel has {} petals.", mode.name(), items.len());
        return 2;
    }
    println!("{} wheel: {:?}; dropping on {}", mode.name(), items.iter().map(|i| i.title()).collect::<Vec<_>>(), if center { "the hub".to_string() } else { items[petal].title() });

    let (cx, cy) = {
        let (x, y) = super::system::cursor_pos();
        let work = super::system::work_area_at(x, y);
        ((work.left + work.right) / 2, (work.top + work.bottom) / 2)
    };
    let start = from.unwrap_or((cx - 220, cy));
    let anchor = (cx, cy);
    let scale = super::system::scale_at(cx, cy);
    let lp = WheelGeometry::default().label_position(petal, items.len());
    let target = if center { anchor } else { (anchor.0 + (lp.x as f32 * scale) as i32, anchor.1 - (lp.y as f32 * scale) as i32) };
    let original = super::system::cursor_pos();

    let result = Rc::new(RefCell::new(None));
    let hwnd = if from.is_some() { HWND::default() } else { window::create(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        WS_POPUP | WS_VISIBLE | WS_BORDER,
        "Satsuma drag test",
        start.0 - 40,
        start.1 - 40,
        80,
        80,
        None,
        Rc::new(Source { files, result: result.clone() }),
    ) };
    let input = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        unsafe {
            let _ = SetCursorPos(start.0, start.1);
        }
        std::thread::sleep(Duration::from_millis(100));
        mouse(MOUSEEVENTF_LEFTDOWN);
        std::thread::sleep(Duration::from_millis(150));
        glide(start, anchor, 12);
        std::thread::sleep(Duration::from_millis(120));
        if let Some(k) = also {
            key(k, false);
            std::thread::sleep(Duration::from_millis(200));
        }
        key(vk, false);
        std::thread::sleep(Duration::from_millis(70));
        if !hold {
            key(vk, true);
        }
        std::thread::sleep(Duration::from_millis(500));
        glide(anchor, target, 10);
        // With a screenshot, hover long enough for the size preview.
        std::thread::sleep(Duration::from_millis(if shot.is_some() { 2500 } else { 450 }));
        if let Some(path) = &shot {
            let half = (150.0 * scale) as i32;
            if let Err(e) = screenshot(anchor.0 - half, anchor.1 - half, half * 2, half * 2, path) {
                println!("screenshot failed: {e}");
            }
        }
        // Only drop on the wheel; anywhere else, cancel so no other app receives the files.
        let under = unsafe { WindowFromPoint(windows::Win32::Foundation::POINT { x: target.0, y: target.1 }) };
        let mut title = [0u16; 64];
        let n = unsafe { GetWindowTextW(under, &mut title) } as usize;
        if String::from_utf16_lossy(&title[..n]) != "Satsuma Wheel" {
            println!("the wheel is not under the pointer; cancelling the drag");
            // Release modifiers first: Ctrl+Esc would open Start.
            if let Some(k) = also {
                key(k, true);
            }
            if hold {
                key(vk, true);
            }
            key(VK_ESCAPE, false);
            key(VK_ESCAPE, true);
            std::thread::sleep(Duration::from_millis(100));
        }
        mouse(MOUSEEVENTF_LEFTUP);
        if hold {
            key(vk, true);
        }
        if let Some(k) = also {
            key(k, true);
        }
        if from.is_some() {
            std::thread::sleep(Duration::from_millis(300));
        }
    });
    if from.is_some() {
        let _ = input.join();
        unsafe {
            let _ = SetCursorPos(original.0, original.1);
        }
        println!("drag from another app finished; see the app's log for what it saw");
        return 0;
    }
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyWindow(hwnd);
    }
    let _ = input.join();
    unsafe {
        let _ = SetCursorPos(original.0, original.1);
    }
    let outcome = result.borrow().clone().unwrap_or_else(|| "no drag".into());
    println!("{outcome}");
    if outcome == "drop effect 1" || (center && outcome == "drop effect 0") {
        0
    } else {
        1
    }
}

/// `satsuma-cli selftest-screenshot "<window title>" <out.png>`: captures a top-level window.
pub fn selftest_screenshot(args: &[String]) -> i32 {
    let (Some(title), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("Usage: satsuma-cli selftest-screenshot <window title> <out.png>");
        return 2;
    };
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let Ok(hwnd) = FindWindowW(None, &windows::core::HSTRING::from(title.as_str())) else {
            eprintln!("No window titled {title}");
            return 1;
        };
        let mut r = windows::Win32::Foundation::RECT::default();
        // The visible frame, without the invisible resize borders.
        if windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(hwnd, windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of_val(&r) as u32).is_err() {
            let _ = GetWindowRect(hwnd, &mut r);
        }
        match screenshot(r.left, r.top, r.right - r.left, r.bottom - r.top, std::path::Path::new(out)) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        }
    }
}
