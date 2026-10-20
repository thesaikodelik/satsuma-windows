//! Satsuma Setup: a per-user installer (no admin rights) in Satsuma's own look — the
//! glass window, a live wheel whose highlight flows from petal to petal, and the citrus
//! accent. The release files ride along at the end of `Satsuma-Setup-<version>.exe`
//! (`stub | zip | zip length (u64 LE) | "SATSUMA1"`); the stub alone, copied into the
//! install folder as "Uninstall Satsuma.exe", is the uninstaller (`--uninstall`).
//!
//! Installs to `%LOCALAPPDATA%\Programs\Satsuma`, where the app's own updater keeps it
//! current, adds Start menu (and optionally desktop) shortcuts, the sign-in entry, and
//! an entry in Settings › Apps.

use super::gfx::{Align, Painter, Rgba, Surface, Weight};
use super::system;
use super::theme::{Palette, ACCENT};
use super::wheel::{self, WheelModel};
use super::window::{self, Handler};
use satsuma_core::geometry::WHEEL_VIEW_SIZE;
use satsuma_core::settings::Settings;
use satsuma_core::{Format, WheelItem, WheelMode};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const MAGIC: &[u8; 8] = b"SATSUMA1";
const TITLE: &str = "Satsuma Setup";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Satsuma";
const UNINSTALLER: &str = "Uninstall Satsuma.exe";
const REPO_URL: &str = "https://github.com/thesaikodelik/satsuma-windows";
const VERSION: &str = env!("CARGO_PKG_VERSION");

const W: f32 = 660.0;
const H: f32 = 400.0;
/// The right-hand column.
const RX: f32 = 344.0;
const RW: f32 = W - RX - 28.0;
const BUTTON_Y: f32 = H - 64.0;
const TIMER: usize = 1;

// ---------------------------------------------------------------------- places

pub fn install_dir() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"));
    local.join("Programs").join("Satsuma")
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = p.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(p.0 as *const _));
        path
    }
}

fn start_menu_link() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Programs).map(|d| d.join("Satsuma.lnk"))
}

fn desktop_link() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Desktop).map(|d| d.join("Satsuma.lnk"))
}

// ---------------------------------------------------------------------- payload

/// The installer's own bytes split into the program (stub) and the release zip.
fn payload() -> Option<(Vec<u8>, Vec<u8>)> {
    let mut bytes = std::fs::read(std::env::current_exe().ok()?).ok()?;
    let n = bytes.len();
    if n < 16 || &bytes[n - 8..] != MAGIC {
        return None;
    }
    let zip_len = u64::from_le_bytes(bytes[n - 16..n - 8].try_into().ok()?) as usize;
    let stub_len = n.checked_sub(16 + zip_len)?;
    let zip = bytes[stub_len..stub_len + zip_len].to_vec();
    bytes.truncate(stub_len);
    Some((bytes, zip))
}

// ---------------------------------------------------------------------- registry

fn reg_key(path: &str) -> Option<HKEY> {
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE | KEY_QUERY_VALUE, None, &mut key, None)
            .ok()
            .ok()?;
    }
    Some(key)
}

fn reg_set(key: HKEY, name: &str, value: &str) {
    let data: Vec<u8> = value.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect();
    unsafe {
        let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(&data));
    }
}

fn reg_set_dword(key: HKEY, name: &str, value: u32) {
    unsafe {
        let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&value.to_le_bytes()));
    }
}

/// The version Settings › Apps lists, if Satsuma was installed with Setup.
pub fn installed_version() -> Option<String> {
    let mut buf = [0u16; 64];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY), &HSTRING::from("DisplayVersion"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size))
            .ok()
            .ok()?;
    }
    let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// Keeps Settings › Apps in step after the app updates itself.
pub fn record_version(version: &str) {
    if installed_version().is_none() {
        return;
    }
    if let Some(key) = reg_key(UNINSTALL_KEY) {
        reg_set(key, "DisplayVersion", version);
        unsafe {
            let _ = RegCloseKey(key);
        }
    }
}

fn register(dir: &Path) {
    let Some(key) = reg_key(UNINSTALL_KEY) else { return };
    let exe = dir.join("satsuma.exe");
    let size_kb = std::fs::read_dir(dir).map(|e| e.flatten().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum::<u64>() / 1024).unwrap_or(0);
    reg_set(key, "DisplayName", "Satsuma");
    reg_set(key, "DisplayVersion", VERSION);
    reg_set(key, "Publisher", "thesaikodelik");
    reg_set(key, "DisplayIcon", &format!("{},0", exe.display()));
    reg_set(key, "InstallLocation", &dir.display().to_string());
    reg_set(key, "UninstallString", &format!("\"{}\" --uninstall", dir.join(UNINSTALLER).display()));
    reg_set(key, "URLInfoAbout", REPO_URL);
    reg_set(key, "HelpLink", REPO_URL);
    reg_set_dword(key, "EstimatedSize", size_kb.min(u32::MAX as u64) as u32);
    reg_set_dword(key, "NoModify", 1);
    reg_set_dword(key, "NoRepair", 1);
    unsafe {
        let _ = RegCloseKey(key);
    }
}

// ---------------------------------------------------------------------- steps

/// Asks a running Satsuma (from any folder) to quit, and waits for it.
fn close_running() {
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> windows::core::BOOL {
        let (mut class, mut title) = ([0u16; 32], [0u16; 32]);
        let c = GetClassNameW(hwnd, &mut class) as usize;
        let t = GetWindowTextW(hwnd, &mut title) as usize;
        if String::from_utf16_lossy(&class[..c]) == "SatsumaWindow" && String::from_utf16_lossy(&title[..t]) == "Satsuma" {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != std::process::id() {
                (*(list.0 as *mut Vec<(HWND, u32)>)).push((hwnd, pid));
            }
        }
        true.into()
    }
    let mut found: Vec<(HWND, u32)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut _ as isize));
        for (hwnd, pid) in found {
            let process = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, false, pid).ok();
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            if let Some(h) = process {
                if WaitForSingleObject(h, 6000).0 != 0 {
                    let _ = TerminateProcess(h, 0);
                    let _ = WaitForSingleObject(h, 2000);
                }
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
        }
    }
}

fn make_link(link: &Path, target: &Path) -> Result<(), String> {
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    unsafe {
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        sl.SetPath(&HSTRING::from(target.as_os_str())).map_err(|e| e.message())?;
        if let Some(dir) = target.parent() {
            let _ = sl.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()));
        }
        let _ = sl.SetDescription(&HSTRING::from("Drag files, tap a key, drop on a petal."));
        let _ = sl.SetIconLocation(&HSTRING::from(target.as_os_str()), 0);
        let file: IPersistFile = sl.cast().map_err(|e| e.message())?;
        file.Save(&HSTRING::from(link.as_os_str()), true).map_err(|e| e.message())
    }
}

/// Writes `bytes` to `to`, moving a locked file (a running exe) aside first.
fn put(to: &Path, bytes: &[u8]) -> Result<(), String> {
    if std::fs::write(to, bytes).is_ok() {
        return Ok(());
    }
    let mut old = to.as_os_str().to_owned();
    old.push(".old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(to, &old).map_err(|e| format!("Couldn't replace {}: {e}", to.display()))?;
    std::fs::write(to, bytes).map_err(|e| format!("Couldn't write {}: {e}", to.display()))
}

#[derive(Clone, Copy)]
struct Options {
    at_login: bool,
    desktop: bool,
    keep_settings: bool,
}

#[derive(Default)]
struct Progress {
    fraction: f32,
    step: String,
    result: Option<Result<(), String>>,
}

type Shared = Arc<Mutex<Progress>>;

fn report(p: &Shared, fraction: f32, step: &str) {
    if let Ok(mut p) = p.lock() {
        p.fraction = fraction;
        p.step = step.to_string();
    }
}

fn install(opts: Options, stub: Vec<u8>, zip: Vec<u8>, p: &Shared) -> Result<(), String> {
    let dir = install_dir();
    report(p, 0.04, "Closing Satsuma if it's open…");
    close_running();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;

    report(p, 0.10, "Copying Satsuma…");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| format!("The installer is damaged ({e})."))?;
    let total: u64 = (0..archive.len()).filter_map(|i| archive.by_index(i).ok().map(|f| f.size())).sum::<u64>().max(1);
    let mut done = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let path = entry.name().map_err(|e| e.to_string())?.to_string();
        let name = path.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        if name.is_empty() {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| e.to_string())?;
        put(&dir.join(&name), &bytes)?;
        done += entry.size();
        report(p, 0.10 + 0.65 * done as f32 / total as f32, &format!("Copying {name}…"));
    }
    put(&dir.join(UNINSTALLER), &stub)?;
    // A download from an older version would only get in the way.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let _ = std::fs::remove_dir_all(PathBuf::from(local).join("Satsuma").join("Updates"));
    }

    report(p, 0.82, "Adding shortcuts…");
    let exe = dir.join("satsuma.exe");
    if let Some(link) = start_menu_link() {
        make_link(&link, &exe)?;
    }
    if let Some(link) = desktop_link() {
        if opts.desktop {
            make_link(&link, &exe)?;
        } else {
            let _ = std::fs::remove_file(link);
        }
    }

    report(p, 0.92, "Registering Satsuma…");
    system::set_launch_at_login_for(&exe, opts.at_login)?;
    register(&dir);
    std::thread::sleep(Duration::from_millis(250));
    report(p, 1.0, "Done");
    Ok(())
}

fn uninstall(opts: Options, p: &Shared) -> Result<(), String> {
    let dir = install_dir();
    report(p, 0.08, "Closing Satsuma…");
    close_running();
    report(p, 0.30, "Removing shortcuts…");
    for link in [start_menu_link(), desktop_link()].into_iter().flatten() {
        let _ = std::fs::remove_file(link);
    }
    let _ = system::set_launch_at_login_for(&dir.join("satsuma.exe"), false);
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY));
    }
    report(p, 0.55, "Removing files…");
    let me = std::env::current_exe().ok();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if Some(e.path()) != me {
                let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
            }
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let _ = std::fs::remove_dir_all(PathBuf::from(local).join("Satsuma"));
    }
    if !opts.keep_settings {
        report(p, 0.80, "Removing settings…");
        if let Some(parent) = Settings::default_path().parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
    }
    std::thread::sleep(Duration::from_millis(250));
    report(p, 1.0, "Done");
    Ok(())
}

/// The uninstaller can't delete itself while running: a hidden shell does it after.
fn remove_leftovers() {
    use std::os::windows::process::CommandExt;
    let dir = install_dir();
    // cmd reads its own quoting, so the line goes as it is (not escaped for C programs).
    let line = format!("/c ping 127.0.0.1 -n 3 > nul & rmdir /s /q \"{}\"", dir.display());
    let _ = std::process::Command::new("cmd")
        .raw_arg(line)
        .current_dir(std::env::temp_dir())
        .creation_flags(0x0800_0000)
        .spawn();
}

// ---------------------------------------------------------------------- window

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Install,
    Uninstall,
}

#[derive(Clone, PartialEq)]
enum Page {
    Welcome,
    Working,
    Done,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Id {
    Primary,
    Secondary,
    /// 0 start at sign-in, 1 desktop shortcut, 2 keep settings.
    Toggle(u8),
}

struct State {
    mode: Mode,
    page: Page,
    opts: Options,
    payload: Option<(Vec<u8>, Vec<u8>)>,
    installed: Option<String>,
    progress: Shared,
    shown: f32,
    hover: Option<Id>,
    pressed: Option<Id>,
    wheel: WheelModel,
    next_petal: Instant,
    page_since: Instant,
    started: Instant,
    scale: f32,
    leftovers: bool,
    logo: Vec<u8>,
}

const LOGO_PX: u32 = 192;

impl State {
    fn toggles(&self) -> Vec<(u8, &'static str, bool)> {
        match (self.mode, &self.page) {
            (Mode::Install, Page::Welcome) => vec![(0, "Start Satsuma when I sign in", self.opts.at_login), (1, "Add a desktop shortcut", self.opts.desktop)],
            (Mode::Uninstall, Page::Welcome) => vec![(2, "Keep my settings", self.opts.keep_settings)],
            _ => Vec::new(),
        }
    }

    fn toggle_y(k: usize) -> f32 {
        204.0 + k as f32 * 40.0
    }

    fn buttons(&self) -> (Option<String>, Option<String>) {
        let install_label = match &self.installed {
            Some(v) if super::update::is_newer(VERSION, v) => format!("Update to {VERSION}"),
            Some(v) if v == VERSION => "Reinstall".to_string(),
            _ => "Install".to_string(),
        };
        match (self.mode, &self.page) {
            (Mode::Install, Page::Welcome) => (Some(install_label), Some("Cancel".into())),
            (Mode::Uninstall, Page::Welcome) => (Some("Remove".into()), Some("Cancel".into())),
            (_, Page::Working) => (None, None),
            (Mode::Install, Page::Done) => (Some("Open Satsuma".into()), Some("Close".into())),
            (Mode::Uninstall, Page::Done) => (Some("Close".into()), None),
            (_, Page::Failed(_)) => (Some("Try Again".into()), Some("Close".into())),
        }
    }

    fn widgets(&self) -> Vec<(Id, (f32, f32, f32, f32))> {
        let mut v = Vec::new();
        for (k, (id, _, _)) in self.toggles().iter().enumerate() {
            v.push((Id::Toggle(*id), (RX, Self::toggle_y(k), RW, 30.0)));
        }
        let (primary, secondary) = self.buttons();
        let pw = if primary.as_deref().is_some_and(|s| s.len() > 9) { 164.0 } else { 132.0 };
        if primary.is_some() {
            v.push((Id::Primary, (W - 28.0 - pw, BUTTON_Y, pw, 36.0)));
        }
        if secondary.is_some() {
            v.push((Id::Secondary, (W - 28.0 - pw - 12.0 - 104.0, BUTTON_Y, 104.0, 36.0)));
        }
        v
    }

    fn hit(&self, x: f32, y: f32) -> Option<Id> {
        self.widgets().into_iter().find(|(_, r)| x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3).map(|(id, _)| id)
    }

    fn set_page(&mut self, page: Page) {
        self.page = page;
        self.page_since = Instant::now();
        self.hover = None;
        self.pressed = None;
    }

    fn start_work(&mut self) {
        self.progress = Arc::new(Mutex::new(Progress::default()));
        self.shown = 0.0;
        self.set_page(Page::Working);
        let (mode, opts, progress) = (self.mode, self.opts, self.progress.clone());
        let payload = self.payload.clone();
        std::thread::spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            }
            let result = match (mode, payload) {
                (Mode::Install, Some((stub, zip))) => install(opts, stub, zip, &progress),
                (Mode::Install, None) => Err("This installer is incomplete. Download it again.".into()),
                (Mode::Uninstall, _) => uninstall(opts, &progress),
            };
            if let Ok(mut p) = progress.lock() {
                p.result = Some(result);
            }
        });
    }

    /// One animation frame: the wheel's highlight walks the petals; progress eases.
    fn step(&mut self) {
        let now = Instant::now();
        let n = self.wheel.items.len().max(1);
        if now >= self.next_petal {
            let next = self.wheel.selected.map(|i| (i + 1) % n).unwrap_or(0);
            self.wheel.selected = Some(next);
            self.wheel.aim_liquid();
            let gap = if self.page == Page::Working { 420 } else { 1100 };
            self.next_petal = now + Duration::from_millis(gap);
        }
        self.wheel.liquid.step(now);
        self.wheel.phase = self.started.elapsed().as_secs_f32();
        if self.page == Page::Working {
            let (target, result) = self.progress.lock().map(|p| (p.fraction, p.result.clone())).unwrap_or((0.0, None));
            self.shown += (target - self.shown) * 0.18;
            match result {
                Some(Ok(())) if self.shown > 0.985 => {
                    self.shown = 1.0;
                    self.set_page(Page::Done);
                    if self.mode == Mode::Uninstall {
                        self.leftovers = true;
                    }
                }
                Some(Err(e)) => self.set_page(Page::Failed(e)),
                _ => {}
            }
        }
        self.wheel.hub = match &self.page {
            Page::Working => vec![format!("{:.0}%", self.shown * 100.0)],
            _ => Vec::new(),
        };
    }

    fn paint(&self, p: &Painter, pal: &Palette) {
        p.fill_round_rect(0.0, 0.0, W, H, 0.0, if pal.glass { pal.backdrop } else { Rgba(pal.panel.0, pal.panel.1, pal.panel.2, 1.0) });

        // The live wheel on a citrus glow.
        let size = WHEEL_VIEW_SIZE as f32;
        let (wx, wy) = (18.0, (H - size) / 2.0);
        let (cx, cy) = (wx + size / 2.0, wy + size / 2.0);
        let breathe = 0.5 + 0.5 * (self.started.elapsed().as_secs_f32() * 1.4).sin();
        let wheel_pal = Palette::current();
        if self.mode == Mode::Install || self.page != Page::Done {
            for k in (1..=9).rev() {
                p.fill_circle(cx, cy, size * 0.36 + k as f32 * 7.0, ACCENT.alpha(0.016 + 0.008 * breathe));
            }
            p.translated(wx, wy, &|p| wheel::paint(p, &self.wheel, &wheel_pal, None, self.scale));
        } else {
            // Removed: the app's icon, fading as a goodbye.
            let fade = (1.0 - self.page_since.elapsed().as_secs_f32() / 1.2).clamp(0.25, 1.0);
            p.image(&self.logo, LOGO_PX, LOGO_PX, (cx - 48.0, cy - 48.0, 96.0, 96.0));
            p.fill_round_rect(cx - 50.0, cy - 50.0, 100.0, 100.0, 22.0, pal.backdrop.alpha(1.0 - fade));
        }

        // Pages slide up and fade in.
        let k = (self.page_since.elapsed().as_secs_f32() / 0.35).min(1.0);
        let k = 1.0 - (1.0 - k).powi(3);
        let dy = (1.0 - k) * 14.0;
        let a = k;
        let label = pal.label.alpha(a);
        let secondary = pal.secondary.alpha(a);
        let tertiary = pal.tertiary.alpha(a);

        match (&self.page, self.mode) {
            (Page::Welcome, Mode::Install) => {
                p.text("Satsuma", (RX, 44.0 + dy, RW, 40.0), 30.0, Weight::Bold, label, Align::Leading, false);
                let sub = match &self.installed {
                    Some(v) if v != VERSION => format!("Version {VERSION}  ·  you have {v}"),
                    _ => format!("Version {VERSION} for Windows"),
                };
                p.text(&sub, (RX, 86.0 + dy, RW, 18.0), 12.0, Weight::Regular, secondary, Align::Leading, false);
                p.text(
                    "Drag files, tap Shift, Ctrl or Alt, and drop them on a petal. Everything is converted right on your PC.",
                    (RX, 118.0 + dy, RW, 60.0),
                    13.0,
                    Weight::Regular,
                    label.alpha(0.9),
                    Align::Leading,
                    true,
                );
                self.paint_toggles(p, pal, a, dy);
                p.text("Installs just for you in %LOCALAPPDATA%\\Programs — no administrator needed. It keeps itself up to date.", (RX, 290.0 + dy, RW, 34.0), 10.5, Weight::Regular, tertiary, Align::Leading, true);
            }
            (Page::Welcome, Mode::Uninstall) => {
                p.text("Remove Satsuma?", (RX, 60.0 + dy, RW, 40.0), 26.0, Weight::Bold, label, Align::Leading, false);
                p.text("This removes the app, its shortcuts, its sign-in entry and downloaded updates.", (RX, 112.0 + dy, RW, 60.0), 13.0, Weight::Regular, label.alpha(0.9), Align::Leading, true);
                self.paint_toggles(p, pal, a, dy);
            }
            (Page::Working, mode) => {
                let title = if mode == Mode::Install { "Installing…" } else { "Removing…" };
                p.text(title, (RX, 70.0 + dy, RW, 40.0), 26.0, Weight::Bold, label, Align::Leading, false);
                let step = self.progress.lock().map(|p| p.step.clone()).unwrap_or_default();
                p.text(&step, (RX, 116.0, RW, 20.0), 12.0, Weight::Regular, secondary, Align::Leading, false);
                // The bar: an orange fill with a light sweeping across it.
                let (bx, by, bw, bh) = (RX, 160.0, RW, 10.0);
                p.fill_round_rect(bx, by, bw, bh, bh / 2.0, pal.control.alpha(if pal.glass { 2.0 } else { 1.0 }));
                let fw = (bw * self.shown).max(bh);
                p.fill_round_rect_gradient(bx, by, fw, bh, bh / 2.0, &[Rgba(1.0, 0.55, 0.20, 1.0), ACCENT], (bx, by), (bx + fw, by));
                let sweep = (self.started.elapsed().as_secs_f32() * 0.8).fract();
                let sx = bx + (fw + 60.0) * sweep - 60.0;
                let (l, r) = (sx.max(bx), (sx + 60.0).min(bx + fw));
                if r > l {
                    p.fill_round_rect_gradient(l, by, r - l, bh, bh / 2.0, &[Rgba(1.0, 1.0, 1.0, 0.0), Rgba(1.0, 1.0, 1.0, 0.45), Rgba(1.0, 1.0, 1.0, 0.0)], (sx, by), (sx + 60.0, by));
                }
                p.text(&format!("{:.0}%", self.shown * 100.0), (RX + RW - 50.0, 178.0, 50.0, 18.0), 11.5, Weight::Semibold, secondary, Align::Center, false);
            }
            (Page::Done, mode) => {
                // A check that draws itself.
                let t = (self.page_since.elapsed().as_secs_f32() / 0.5).min(1.0);
                let (ccx, ccy, r) = (RX + 24.0, 72.0, 24.0);
                p.fill_circle(ccx, ccy, r * (0.6 + 0.4 * t.min(1.0)), ACCENT.alpha(0.9));
                let pts = [(ccx - 10.0, ccy + 1.0), (ccx - 3.0, ccy + 8.0), (ccx + 11.0, ccy - 7.0)];
                let first = (t * 2.0).min(1.0);
                let second = ((t - 0.5) * 2.0).clamp(0.0, 1.0);
                let lerp = |a: (f32, f32), b: (f32, f32), k: f32| (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k);
                let m = lerp(pts[0], pts[1], first);
                p.line(pts[0].0, pts[0].1, m.0, m.1, Rgba::WHITE, 3.0);
                if second > 0.0 {
                    let e = lerp(pts[1], pts[2], second);
                    p.line(pts[1].0, pts[1].1, e.0, e.1, Rgba::WHITE, 3.0);
                }
                if mode == Mode::Install {
                    p.text("Satsuma is ready", (RX, 112.0 + dy, RW, 40.0), 26.0, Weight::Bold, label, Align::Leading, false);
                    p.text(
                        "Its citrus icon lives in the notification area, at the right end of the taskbar. Drag files and tap Shift, Ctrl or Alt to open a wheel.",
                        (RX, 158.0 + dy, RW, 80.0),
                        13.0,
                        Weight::Regular,
                        label.alpha(0.9),
                        Align::Leading,
                        true,
                    );
                    p.text("Find it later in the Start menu, and in Settings › About.", (RX, 252.0 + dy, RW, 34.0), 11.0, Weight::Regular, tertiary, Align::Leading, true);
                } else {
                    p.text("Satsuma was removed", (RX, 112.0 + dy, RW, 40.0), 26.0, Weight::Bold, label, Align::Leading, false);
                    p.text("Thanks for trying it.", (RX, 158.0 + dy, RW, 40.0), 13.0, Weight::Regular, label.alpha(0.9), Align::Leading, true);
                }
            }
            (Page::Failed(e), _) => {
                p.text("Something went wrong", (RX, 60.0 + dy, RW, 40.0), 24.0, Weight::Bold, label, Align::Leading, false);
                p.text(e, (RX, 106.0 + dy, RW, 120.0), 12.5, Weight::Regular, pal.error.alpha(a), Align::Leading, true);
            }
        }
        self.paint_buttons(p, pal);
    }

    fn paint_toggles(&self, p: &Painter, pal: &Palette, a: f32, dy: f32) {
        for (k, (id, text, on)) in self.toggles().into_iter().enumerate() {
            let y = Self::toggle_y(k) + dy;
            p.text(text, (RX, y, RW - 56.0, 30.0), 12.5, Weight::Regular, pal.label.alpha(a), Align::Leading, false);
            let (sx, sy, sw, sh) = (RX + RW - 44.0, y + 3.0, 44.0, 24.0);
            let radius = sh / 2.0;
            let lit = self.hover == Some(Id::Toggle(id));
            if on {
                p.fill_round_rect(sx, sy, sw, sh, radius, ACCENT.alpha(a));
            } else {
                p.fill_round_rect(sx, sy, sw, sh, radius, pal.control.alpha(if lit { 2.4 } else if pal.glass { 1.6 } else { 1.0 }));
                p.stroke_round_rect(sx + 0.5, sy + 0.5, sw - 1.0, sh - 1.0, radius - 0.5, if pal.glass { pal.rim } else { pal.tertiary }, 1.0);
            }
            let knob = if on { sx + sw - radius } else { sx + radius };
            p.fill_circle(knob, sy + radius, radius - 4.0, if on { Rgba::WHITE } else { pal.secondary });
        }
    }

    fn paint_buttons(&self, p: &Painter, pal: &Palette) {
        let (primary, secondary) = self.buttons();
        for (id, r) in self.widgets() {
            let down = self.pressed == Some(id);
            let lit = self.hover == Some(id);
            let radius = r.3 / 2.0;
            match id {
                Id::Primary => {
                    let c = if down { Rgba(0.85, 0.36, 0.07, 1.0) } else if lit { Rgba(1.0, 0.52, 0.20, 1.0) } else { ACCENT };
                    // A soft orange shadow under the main button.
                    p.fill_round_rect(r.0 + 2.0, r.1 + 4.0, r.2 - 4.0, r.3, radius, ACCENT.alpha(0.18));
                    p.fill_round_rect(r.0, r.1, r.2, r.3, radius, c);
                    p.fill_round_rect_gradient(r.0, r.1, r.2, r.3 / 2.0, radius, &[Rgba(1.0, 1.0, 1.0, 0.18), Rgba(1.0, 1.0, 1.0, 0.0)], (r.0, r.1), (r.0, r.1 + r.3 / 2.0));
                    p.text(primary.as_deref().unwrap_or(""), r, 13.0, Weight::Semibold, Rgba::WHITE, Align::Center, false);
                }
                Id::Secondary => {
                    p.fill_round_rect(r.0, r.1, r.2, r.3, radius, pal.control.alpha(if lit || down { 2.4 } else if pal.glass { 1.4 } else { 1.0 }));
                    p.stroke_round_rect(r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, radius - 0.5, if pal.glass { pal.rim } else { pal.separator }, 1.0);
                    p.text(secondary.as_deref().unwrap_or(""), r, 13.0, Weight::Semibold, pal.label, Align::Center, false);
                }
                Id::Toggle(_) => {}
            }
        }
    }
}

struct Messages {
    state: Rc<RefCell<State>>,
}

fn scale_of(hwnd: HWND) -> f32 {
    (unsafe { GetDpiForWindow(hwnd) }.max(96)) as f32 / 96.0
}

fn launch_installed() {
    let exe = install_dir().join("satsuma.exe");
    system::open_item(&exe);
}

impl Messages {
    fn click(&self, hwnd: HWND, id: Id) {
        // Decide with the state borrowed, act after: closing the window reads the state.
        let close = {
            let mut s = self.state.borrow_mut();
            match id {
                Id::Toggle(0) => {
                    s.opts.at_login = !s.opts.at_login;
                    false
                }
                Id::Toggle(1) => {
                    s.opts.desktop = !s.opts.desktop;
                    false
                }
                Id::Toggle(_) => {
                    s.opts.keep_settings = !s.opts.keep_settings;
                    false
                }
                Id::Primary => match (&s.page, s.mode) {
                    (Page::Welcome, _) | (Page::Failed(_), _) => {
                        s.start_work();
                        false
                    }
                    (Page::Done, Mode::Install) => {
                        launch_installed();
                        true
                    }
                    _ => true,
                },
                Id::Secondary => true,
            }
        };
        if close {
            unsafe {
                let _ = DestroyWindow(hwnd);
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
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.step();
                }
                if !unsafe { IsIconic(hwnd) }.as_bool() {
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = dip(lparam);
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.hover = s.hit(x, y);
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR => {
                let over = self.state.try_borrow().map(|s| s.hover.is_some()).unwrap_or(false);
                if over && window::loword(lparam.0) as u32 == HTCLIENT {
                    unsafe {
                        SetCursor(LoadCursorW(None, IDC_HAND).ok());
                    }
                    return Some(LRESULT(1));
                }
                None
            }
            WM_LBUTTONDOWN => {
                let (x, y) = dip(lparam);
                if let Ok(mut s) = self.state.try_borrow_mut() {
                    s.pressed = s.hit(x, y);
                }
                unsafe { SetCapture(hwnd) };
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                let (x, y) = dip(lparam);
                let clicked = self.state.try_borrow_mut().ok().and_then(|mut s| {
                    let p = s.pressed.take();
                    p.filter(|p| s.hit(x, y) == Some(*p))
                });
                if let Some(id) = clicked {
                    self.click(hwnd, id);
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                let (primary, secondary) = self.state.try_borrow().map(|s| s.buttons()).unwrap_or((None, None));
                if vk == VK_RETURN && primary.is_some() {
                    self.click(hwnd, Id::Primary);
                } else if vk == VK_ESCAPE && secondary.is_some() {
                    self.click(hwnd, Id::Secondary);
                }
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                // Not halfway through copying files.
                let working = self.state.try_borrow().map(|s| s.page == Page::Working).unwrap_or(false);
                if working {
                    Some(LRESULT(0))
                } else {
                    None
                }
            }
            WM_DPICHANGED => {
                let r = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                super::editor::apply_theme(hwnd);
                None
            }
            WM_DESTROY => {
                if self.state.try_borrow().map(|s| s.leftovers).unwrap_or(false) {
                    remove_leftovers();
                }
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

/// Runs Setup (or, with `--uninstall`, the uninstaller); returns the exit code.
pub fn run() -> i32 {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let mode = if std::env::args().any(|a| a == "--uninstall") { Mode::Uninstall } else { Mode::Install };
    // The look follows the person's Satsuma settings when there are any.
    let settings = Settings::load(&Settings::default_path());
    super::theme::set_appearance(settings.appearance);
    super::theme::set_transparency(settings.transparency);

    let items = [Format::Png, Format::Webp, Format::Heic, Format::Pdf, Format::Avif, Format::Docx, Format::Jpg, Format::Tiff]
        .into_iter()
        .map(WheelItem::Convert)
        .collect();
    let mut wheel = WheelModel::new(WheelMode::Conversions, items);
    wheel.on_glass_window = true;
    wheel.selected = Some(0);
    wheel.settle_liquid();

    let payload = if mode == Mode::Install { payload() } else { None };
    let state = Rc::new(RefCell::new(State {
        mode,
        page: Page::Welcome,
        opts: Options { at_login: true, desktop: false, keep_settings: true },
        payload,
        installed: installed_version(),
        progress: Arc::new(Mutex::new(Progress::default())),
        shown: 0.0,
        hover: None,
        pressed: None,
        wheel,
        next_petal: Instant::now() + Duration::from_millis(900),
        page_since: Instant::now(),
        started: Instant::now(),
        scale: 1.0,
        leftovers: false,
        logo: super::icon::app_icon(LOGO_PX),
    }));

    let (cx, cy) = system::cursor_pos();
    let scale = system::scale_at(cx, cy);
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let ex = WINDOW_EX_STYLE(0);
    let size = |s: f32| {
        let mut r = RECT { left: 0, top: 0, right: (W * s).round() as i32, bottom: (H * s).round() as i32 };
        unsafe {
            let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, (s * 96.0) as u32);
        }
        (r.right - r.left, r.bottom - r.top)
    };
    let (ww, wh) = size(scale);
    let work = system::work_area_at(cx, cy);
    let x = (work.left + work.right - ww) / 2;
    let y = ((work.top + work.bottom - wh) / 2).max(work.top);
    let title = if mode == Mode::Uninstall { "Uninstall Satsuma" } else { TITLE };
    let hwnd = window::create(ex, style, title, x, y, ww, wh, None, Rc::new(Messages { state }));
    super::editor::apply_theme(hwnd);
    let s = scale_of(hwnd);
    if (s - scale).abs() > 0.01 {
        let (w2, h2) = size(s);
        unsafe {
            let _ = SetWindowPos(hwnd, None, 0, 0, w2, h2, SWP_NOMOVE | SWP_NOZORDER);
        }
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
        SetTimer(Some(hwnd), TIMER, 16, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    0
}
