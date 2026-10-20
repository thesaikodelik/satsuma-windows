//! Windows services: pointer and keyboard state, monitors, theme, clipboard, terminals,
//! Share, file dialogs, the highlight sound, Launch at Login and shell icons.

use satsuma_core::naming::first_free;
use satsuma_core::settings::TerminalApp;
use satsuma_core::Mods;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::core::{Interface, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{CF_HDROP, CF_UNICODETEXT, DROPEFFECT_COPY};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
}

/// The primary mouse button is held (respects swapped buttons).
pub fn left_mouse_down() -> bool {
    let swapped = unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0;
    down(if swapped { VK_RBUTTON } else { VK_LBUTTON })
}

/// Held modifier keys: Alt is reported as Option and the Windows key as Command.
pub fn current_mods() -> Mods {
    let mut m = Mods::NONE;
    if down(VK_SHIFT) {
        m = m.union(Mods::SHIFT);
    }
    // AltGr (right Alt on many layouts) arrives as Left Ctrl + Right Alt: count it as Alt.
    let altgr = down(VK_RMENU) && down(VK_LCONTROL) && !down(VK_RCONTROL);
    if down(VK_CONTROL) && !altgr {
        m = m.union(Mods::CONTROL);
    }
    if down(VK_MENU) {
        m = m.union(Mods::OPTION);
    }
    if down(VK_LWIN) || down(VK_RWIN) {
        m = m.union(Mods::COMMAND);
    }
    m
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Nudges the pointer by zero so a drag loop re-checks the window under it.
pub fn nudge_pointer() {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 } },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

/// The work area (screen minus taskbar) of the monitor at a point, in pixels.
pub fn work_area_at(x: i32, y: i32) -> RECT {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        info.rcWork
    }
}

/// Pixels per DIP on the monitor at a point (1.0 at 100 % scaling).
pub fn scale_at(x: i32, y: i32) -> f32 {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() {
            dx as f32 / 96.0
        } else {
            1.0
        }
    }
}

fn reg_dword(root: HKEY, path: &str, name: &str) -> Option<u32> {
    let mut data = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(root, &HSTRING::from(path), &HSTRING::from(name), RRF_RT_REG_DWORD, None, Some(&mut data as *mut u32 as *mut _), Some(&mut size))
    };
    r.is_ok().then_some(data)
}

/// Apps use the dark theme (Settings › Personalization › Colors).
pub fn is_dark() -> bool {
    reg_dword(HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme") == Some(0)
}

/// The taskbar uses the dark theme (for the notification-area icon colour).
pub fn taskbar_is_dark() -> bool {
    reg_dword(HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "SystemUsesLightTheme") != Some(1)
}

pub fn alert(title: &str, message: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(message), &HSTRING::from(title), MB_OK | MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND);
    }
}

// ------------------------------------------------------------------ clipboard

fn global_from(bytes: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).ok()?;
        let ptr = GlobalLock(mem) as *mut u8;
        if ptr.is_null() {
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        let _ = GlobalUnlock(mem);
        Some(mem)
    }
}

fn with_clipboard(f: impl FnOnce()) {
    unsafe {
        // Another app may hold the clipboard for a moment.
        for _ in 0..10 {
            if OpenClipboard(None).is_ok() {
                let _ = EmptyClipboard();
                f();
                let _ = CloseClipboard();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

/// A `DROPFILES` block (`CF_HDROP`) for `paths`.
pub fn hdrop_bytes(paths: &[PathBuf]) -> Vec<u8> {
    let mut bytes = Vec::new();
    // DROPFILES { pFiles = 20, pt = (0, 0), fNC = 0, fWide = 1 }
    bytes.extend_from_slice(&20u32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    for p in paths {
        for u in p.as_os_str().encode_wide_compat() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        bytes.extend_from_slice(&0u16.to_le_bytes());
    }
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes
}

trait EncodeWide {
    fn encode_wide_compat(&self) -> Vec<u16>;
}

impl EncodeWide for std::ffi::OsStr {
    fn encode_wide_compat(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().collect()
    }
}

/// Files on the clipboard, ready to paste in File Explorer.
pub fn copy_files(paths: &[PathBuf]) {
    with_clipboard(|| unsafe {
        if let Some(mem) = global_from(&hdrop_bytes(paths)) {
            let _ = SetClipboardData(CF_HDROP.0 as u32, Some(HANDLE(mem.0)));
        }
        let effect = RegisterClipboardFormatW(&HSTRING::from("Preferred DropEffect"));
        if let Some(mem) = global_from(&DROPEFFECT_COPY.0.to_le_bytes()) {
            let _ = SetClipboardData(effect, Some(HANDLE(mem.0)));
        }
    });
}

/// Full paths as text, one per line.
pub fn copy_paths(paths: &[PathBuf]) {
    let text = satsuma_core::actions::paths_as_text(paths).replace('\n', "\r\n");
    let mut wide: Vec<u8> = Vec::new();
    for u in text.encode_utf16().chain(std::iter::once(0)) {
        wide.extend_from_slice(&u.to_le_bytes());
    }
    with_clipboard(|| unsafe {
        if let Some(mem) = global_from(&wide) {
            let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(mem.0)));
        }
    });
}

// ------------------------------------------------------------------ terminals

pub fn open_terminal(path: &Path, app: TerminalApp) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x10;
    let dir = satsuma_core::actions::terminal_directory(path, path.is_dir());
    let result = match app {
        TerminalApp::PowerShell => {
            let shell = if which("pwsh.exe").is_some() { "pwsh.exe" } else { "powershell.exe" };
            std::process::Command::new(shell).arg("-NoExit").current_dir(&dir).creation_flags(CREATE_NEW_CONSOLE).spawn()
        }
        TerminalApp::Cmd => std::process::Command::new("cmd.exe").current_dir(&dir).creation_flags(CREATE_NEW_CONSOLE).spawn(),
        _ => match which("wt.exe") {
            Some(wt) => std::process::Command::new(wt).arg("-d").arg(&dir).spawn(),
            None => return Err("Windows Terminal is not installed. Get it from the Microsoft Store, or choose PowerShell or Command Prompt under Terminal App.".into()),
        },
    };
    result.map(|_| ()).map_err(|e| format!("Couldn't open {}: {e}", app.name()))
}

fn which(exe: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    // App execution aliases (wt.exe) live here even when PATH misses it.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join(r"Microsoft\WindowsApps"));
    }
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.exists())
}

// ------------------------------------------------------------------ dialogs

unsafe fn dialog_results(dialog: &IFileOpenDialog) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(items) = dialog.GetResults() {
        let count = items.GetCount().unwrap_or(0);
        for i in 0..count {
            if let Ok(item) = items.GetItemAt(i) {
                if let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                    out.push(PathBuf::from(name.to_string().unwrap_or_default()));
                    CoTaskMemFree(Some(name.0 as *const _));
                }
            }
        }
    }
    out
}

fn open_dialog(title: &str, button: Option<&str>, options: FILEOPENDIALOGOPTIONS, filter: Option<(&str, &str)>) -> Vec<PathBuf> {
    unsafe {
        let Ok(dialog): windows::core::Result<IFileOpenDialog> = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) else {
            return Vec::new();
        };
        let current = dialog.GetOptions().unwrap_or_default();
        let _ = dialog.SetOptions(current | options | FOS_FORCEFILESYSTEM);
        let _ = dialog.SetTitle(&HSTRING::from(title));
        if let Some(b) = button {
            let _ = dialog.SetOkButtonLabel(&HSTRING::from(b));
        }
        let (name, spec) = filter.map(|(n, s)| (HSTRING::from(n), HSTRING::from(s))).unwrap_or_default();
        if filter.is_some() {
            let specs = [windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) }];
            let _ = dialog.SetFileTypes(&specs);
        }
        if dialog.Show(None).is_err() {
            return Vec::new();
        }
        dialog_results(&dialog)
    }
}

pub fn choose_files() -> Vec<PathBuf> {
    open_dialog("Convert Files", Some("Choose"), FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST, None)
}

pub fn choose_folder(title: &str, button: &str) -> Option<PathBuf> {
    open_dialog(title, Some(button), FOS_PICKFOLDERS, None).into_iter().next()
}

pub fn choose_image() -> Option<PathBuf> {
    open_dialog(
        "Choose image background",
        Some("Choose"),
        FOS_FILEMUSTEXIST,
        Some(("Images", "*.jpg;*.jpeg;*.png;*.webp;*.heic;*.heif;*.tif;*.tiff;*.bmp;*.gif;*.avif")),
    )
    .into_iter()
    .next()
}

/// The colour picker; returns the chosen colour.
pub fn choose_color(owner: HWND, initial: (u8, u8, u8)) -> Option<(u8, u8, u8)> {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_ANYCOLOR, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
    static mut CUSTOM: [COLORREF; 16] = [COLORREF(0xFFFFFF); 16];
    let rgb = |c: (u8, u8, u8)| COLORREF(c.0 as u32 | (c.1 as u32) << 8 | (c.2 as u32) << 16);
    let mut cc = CHOOSECOLORW {
        lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: owner,
        rgbResult: rgb(initial),
        lpCustColors: std::ptr::addr_of_mut!(CUSTOM) as *mut COLORREF,
        Flags: CC_ANYCOLOR | CC_FULLOPEN | CC_RGBINIT,
        ..Default::default()
    };
    unsafe {
        if ChooseColorW(&mut cc).as_bool() {
            let c = cc.rgbResult.0;
            Some(((c & 0xff) as u8, ((c >> 8) & 0xff) as u8, ((c >> 16) & 0xff) as u8))
        } else {
            None
        }
    }
}

// ------------------------------------------------------------------ move

fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// Moves items into `dest`, never overwriting (name clashes get " 2", " 3" …).
pub fn move_into(paths: &[PathBuf], dest: &Path) -> Vec<String> {
    let mut errors = Vec::new();
    for src in paths {
        let name = src.file_name().unwrap_or_default().to_string_lossy().into_owned();
        if src.parent() == Some(dest) {
            continue;
        }
        let (base, ext) = if src.is_dir() {
            (name.clone(), None)
        } else {
            let stem = satsuma_core::naming::stem_of(&name).to_string();
            let ext = name[stem.len()..].trim_start_matches('.').to_string();
            (stem, if ext.is_empty() { None } else { Some(ext) })
        };
        let target = dest.join(first_free(&base, ext.as_deref(), &|n| dest.join(n).exists()));
        if std::fs::rename(src, &target).is_ok() {
            continue;
        }
        // Different drive: copy, then remove the original.
        match copy_recursive(src, &target) {
            Ok(()) => {
                let removed = if src.is_dir() { std::fs::remove_dir_all(src) } else { std::fs::remove_file(src) };
                if let Err(e) = removed {
                    errors.push(format!("Copied {name} but couldn't remove the original: {e}"));
                }
            }
            Err(e) => {
                let _ = if target.is_dir() { std::fs::remove_dir_all(&target) } else { std::fs::remove_file(&target) };
                errors.push(format!("Couldn't move {name}: {e}"));
            }
        }
    }
    errors
}

// ------------------------------------------------------------------ Share

/// Opens the Windows Share sheet (Nearby Sharing, Mail, Phone Link …) for the items.
pub fn share(owner: HWND, paths: &[PathBuf]) -> Result<(), String> {
    use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::{IStorageItem, StorageFile, StorageFolder};
    use windows::Win32::UI::Shell::IDataTransferManagerInterop;

    let fail = |e: windows::core::Error| format!("Couldn't open Share: {}", e.message());
    let mut items: Vec<IStorageItem> = Vec::new();
    for p in paths {
        let h = HSTRING::from(p.as_os_str());
        let item: windows::core::Result<IStorageItem> = if p.is_dir() {
            StorageFolder::GetFolderFromPathAsync(&h).and_then(|op| op.join()).and_then(|f| f.cast())
        } else {
            StorageFile::GetFileFromPathAsync(&h).and_then(|op| op.join()).and_then(|f| f.cast())
        };
        items.push(item.map_err(fail)?);
    }
    let title = if paths.len() == 1 {
        paths[0].file_name().unwrap_or_default().to_string_lossy().into_owned()
    } else {
        format!("{} items", paths.len())
    };
    unsafe {
        let interop: IDataTransferManagerInterop =
            windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>().map_err(fail)?;
        let manager: DataTransferManager = interop.GetForWindow(owner).map_err(fail)?;
        // One handler per share; the previous one is removed first.
        static TOKEN: std::sync::Mutex<Option<i64>> = std::sync::Mutex::new(None);
        if let Some(old) = TOKEN.lock().unwrap().take() {
            let _ = manager.RemoveDataRequested(old);
        }
        // Storage items are agile WinRT objects, safe to hand to the handler's thread.
        struct Items(Vec<IStorageItem>);
        unsafe impl Send for Items {}
        unsafe impl Sync for Items {}
        impl Items {
            fn list(&self) -> windows_collections::IIterable<IStorageItem> {
                self.0.iter().cloned().map(Some).collect::<Vec<_>>().into()
            }
        }
        let items = Items(items);
        let handler = TypedEventHandler::<DataTransferManager, DataRequestedEventArgs>::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                let data = args.Request()?.Data()?;
                data.Properties()?.SetTitle(&HSTRING::from(title.as_str()))?;
                let list = items.list();
                data.SetStorageItems(&list, true)?;
            }
            Ok(())
        });
        let token = manager.DataRequested(&handler).map_err(fail)?;
        *TOKEN.lock().unwrap() = Some(token);
        interop.ShowShareUIForWindow(owner).map_err(fail)?;
    }
    Ok(())
}

// ------------------------------------------------------------------ sound

/// A short, quiet click (like macOS "Tink" at 25 %), as an in-memory WAV.
fn tick_wav() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| {
        let rate = 44_100u32;
        let samples: Vec<i16> = (0..(rate as usize * 18 / 1000))
            .map(|i| {
                let t = i as f32 / rate as f32;
                let env = (-t * 320.0).exp();
                let s = (2.0 * std::f32::consts::PI * 2600.0 * t).sin() * 0.55 + (2.0 * std::f32::consts::PI * 5200.0 * t).sin() * 0.2;
                (s * env * 0.25 * i16::MAX as f32) as i16
            })
            .collect();
        let data_len = (samples.len() * 2) as u32;
        let mut w = Vec::with_capacity(44 + data_len as usize);
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * 2).to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            w.extend_from_slice(&s.to_le_bytes());
        }
        w
    })
}

pub fn highlight_feedback() {
    unsafe {
        let _ = PlaySoundW(PCWSTR(tick_wav().as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}

// ------------------------------------------------------------------ Launch at Login

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

fn exe_command() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    format!("\"{}\"", exe.display())
}

pub fn launch_at_login() -> bool {
    let mut size = 0u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &HSTRING::from("Satsuma"), RRF_RT_REG_SZ, None, None, Some(&mut size)).is_ok()
    }
}

pub fn set_launch_at_login(on: bool) -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None)
            .ok()
            .map_err(|e| e.message())?;
        let result = if on {
            let value: Vec<u8> = exe_command().encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect();
            RegSetValueExW(key, &HSTRING::from("Satsuma"), None, REG_SZ, Some(&value))
        } else {
            RegDeleteValueW(key, &HSTRING::from("Satsuma"))
        };
        let _ = RegCloseKey(key);
        result.ok().map_err(|e| e.message())
    }
}

// ------------------------------------------------------------------ shell

/// The shell's icon (or thumbnail) for a file, as straight RGBA.
pub fn shell_icon(path: &Path, px: i32) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let bmp = factory.GetImage(SIZE { cx: px, cy: px }, SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK).ok()?;
        let pixels = bitmap_rgba(bmp);
        let _ = DeleteObject(bmp.into());
        pixels
    }
}

/// Reads a 32-bit HBITMAP (premultiplied, as the shell makes them) as straight RGBA.
unsafe fn bitmap_rgba(bmp: HBITMAP) -> Option<(Vec<u8>, u32, u32)> {
    let mut info = BITMAP::default();
    if GetObjectW(bmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _)) == 0 {
        return None;
    }
    let (w, h) = (info.bmWidth, info.bmHeight.abs());
    let mut bi = BITMAPINFO {
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
    let dc = GetDC(None);
    let rows = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    if rows == 0 {
        return None;
    }
    let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
    let mut out = Vec::with_capacity(buf.len());
    for p in buf.chunks_exact(4) {
        let a = if has_alpha { p[3] } else { 255 };
        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
        out.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), a]);
    }
    Some((out, w as u32, h as u32))
}

pub fn open_item(path: &Path) {
    unsafe {
        ShellExecuteW(None, &HSTRING::from("open"), &HSTRING::from(path.as_os_str()), None, None, SW_SHOWNORMAL);
    }
}
