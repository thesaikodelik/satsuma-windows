//! The notification-area app: tray icon and menu, drag monitoring, wheel, jobs and settings.
//! The Windows counterpart of `app.rs`; the job queue, HUD summaries and menu follow it.
//!
//! Drag detection differs from macOS, which watches the shared drag pasteboard. Windows has
//! no such thing, so while the mouse button is held, tapping a wheel key puts an invisible
//! probe (the wheel window itself, an OLE drop target) under the pointer. A drag carrying
//! files reports itself to the probe through `IDropTarget::DragEnter`, and the wheel appears.
//! Text selections, window moves and drags without files never show anything.

use super::editor::EditorWindow;
use super::events::{self, Point, UiEvent};
use super::float::FloatWindow;
use super::hud::{Hud, HudModel};
use super::system;
use super::wheel::{WheelModel, WheelWindow};
use super::window::{self, Handler};
use satsuma_core::settings::{FloatBehavior, Settings};
use satsuma_core::wheel::{initial_keyboard_mode, wheel_items};
use satsuma_core::{Action, Mods, WheelItem, WheelMode};
use satsuma_engine::{capability, detect_format, plan, run_plan, BatchProgress, BatchReport, Cancel, Context, Env, Operation};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Ole::OleInitialize;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// Menu command ids.
const CMD_CONVERT: u32 = 1;
const CMD_HELP: u32 = 2;
const CMD_QUIT: u32 = 3;
const CMD_SETTINGS: u32 = 6;
const CMD_UPDATE: u32 = 7;

const WM_TRAY: u32 = WM_APP + 1;
const TIMER_TICK: usize = 1;
/// 10 ms rounds to the system's ~15.6 ms tick: about 64 frames a second.
const FAST_TICK_MS: u32 = 10;
const IDLE_TICK_MS: u32 = 50;
/// How long the probe waits for a drag to reach it.
const PROBE_TIMEOUT: Duration = Duration::from_millis(650);
/// Probes per mouse press, so a long text selection with Shift held stays cheap.
const MAX_PROBES: u32 = 12;

enum JobMsg {
    Progress(BatchProgress),
    Done { report: BatchReport, skipped: Vec<(PathBuf, String)> },
    Failed(String),
}

struct RunningJob {
    cancel: Cancel,
    rx: Receiver<JobMsg>,
    title: String,
}

struct PendingJob {
    op: Operation,
    files: Vec<PathBuf>,
    at: Point,
}

/// Work that shows modal UI, so it runs after the controller borrow is released.
enum Deferred {
    Alert(String, String),
    Move(Vec<PathBuf>),
    Share(Vec<PathBuf>),
    Welcome,
}

/// Where the drag gesture is, between mouse-down and mouse-up.
#[derive(Debug)]
enum Drag {
    /// Mouse up, or down without a wheel key tapped yet.
    Idle,
    /// The invisible probe is under the pointer, waiting for the drag loop to report.
    Probing { mode: WheelMode, since: Instant },
    /// A file drag with the wheel showing.
    Open { mode: WheelMode, files: Vec<PathBuf> },
    /// Not a file drag; ignore keys until the button comes up.
    Rejected,
}

struct Tray {
    hwnd: HWND,
    icon: HICON,
}

impl Tray {
    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: self.icon,
            ..Default::default()
        }
    }

    fn set_tip(nid: &mut NOTIFYICONDATAW, tip: &str) {
        for (d, s) in nid.szTip.iter_mut().zip(tip.encode_utf16().chain(std::iter::once(0)).take(127)) {
            *d = s;
        }
    }

    fn add(&self, tip: &str) {
        let mut nid = self.data();
        Self::set_tip(&mut nid, tip);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
    }

    fn update(&self, tip: &str) {
        let mut nid = self.data();
        Self::set_tip(&mut nid, tip);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// A Windows notification from the tray icon; clicking it comes back as
    /// `NIN_BALLOONUSERCLICK`.
    fn notify(&self, title: &str, text: &str) {
        let mut nid = self.data();
        nid.uFlags = NIF_INFO;
        for (d, s) in nid.szInfoTitle.iter_mut().zip(title.encode_utf16().chain(std::iter::once(0)).take(63)) {
            *d = s;
        }
        for (d, s) in nid.szInfo.iter_mut().zip(text.encode_utf16().chain(std::iter::once(0)).take(255)) {
            *d = s;
        }
        // The app's own icon, large, instead of the generic "i".
        let icon = super::icon::hicon(&super::icon::app_icon(64), 64);
        match icon {
            Some(h) => {
                nid.hBalloonIcon = h;
                nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
            }
            None => nid.dwInfoFlags = NIIF_INFO,
        }
        unsafe {
            // The icon stays alive: the shell may draw the notification after this returns
            // (one small icon per downloaded update).
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    fn remove(&self) {
        let nid = self.data();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

fn tray_icon_for_theme() -> HICON {
    let px = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16) as u32;
    super::icon::hicon(&super::icon::tray_icon(px, system::taskbar_is_dark()), px).unwrap_or_default()
}

struct Controller {
    settings: Settings,
    settings_path: PathBuf,
    env: Arc<Env>,
    main: HWND,
    tray: Tray,
    wheel: WheelWindow,
    keyboard_files: Option<Vec<PathBuf>>,
    drag: Drag,
    prev_mods: Mods,
    /// Where the mouse button went down, for the "key already held" case.
    press: Option<Point>,
    /// Where the last probe of this press appeared, and how many there were.
    probe_from: Option<Point>,
    probes: u32,
    hud: Hud,
    hud_hide_at: Option<Instant>,
    /// When the mouse comes up over a petal, the drop arrives just after; keep the wheel briefly.
    wheel_hide_at: Option<Instant>,
    floats: Vec<FloatWindow>,
    editors: Vec<EditorWindow>,
    /// Output-size previews for the open wheel.
    estimates: Option<super::estimates::Session>,
    job: Option<RunningJob>,
    queue: Vec<PendingJob>,
    deferred: Vec<Deferred>,
    /// Debug builds: quit once the queue drains (used by SATSUMA_SELFTEST_DROP).
    exit_when_idle: bool,
    /// The tick's current period: fast while something can happen, slow when idle.
    tick_ms: u32,
    /// A downloaded update waiting for a restart (its version).
    update_ready: Option<String>,
}

thread_local! {
    static CONTROLLER: RefCell<Option<Controller>> = const { RefCell::new(None) };
}

/// Runs `f` on the controller unless it is already in use (re-entrant call).
fn with_controller<R>(f: impl FnOnce(&mut Controller) -> R) -> Option<R> {
    CONTROLLER.with(|c| c.try_borrow_mut().ok().and_then(|mut c| c.as_mut().map(f)))
}

fn welcome_text(s: &Settings) -> String {
    let w = &s.wheels;
    format!(
        "Satsuma lives in the notification area, at the right end of the taskbar (it may be under the ^ arrow).\n\n\
         1. Drag files from File Explorer or the desktop.\n\
         2. While dragging, tap {} to convert, {} for tools, or {} for file actions.\n\
         3. Drop onto a petal. Results are saved next to the originals.\n\n\
         You can change the keys, the look and more in Satsuma's Settings.",
        w.conversions.label(),
        w.tools.label(),
        w.actions.label()
    )
}

fn run_deferred() {
    let work = with_controller(|c| std::mem::take(&mut c.deferred)).unwrap_or_default();
    for item in work {
        match item {
            Deferred::Alert(title, message) => system::alert(&title, &message),
            Deferred::Welcome => {
                let text = with_controller(|c| welcome_text(&c.settings)).unwrap_or_default();
                system::alert("Satsuma is ready", &text);
            }
            Deferred::Move(files) => {
                if let Some(dest) = system::choose_folder("Choose where to move the items", "Move Here") {
                    let errors = system::move_into(&files, &dest);
                    if !errors.is_empty() {
                        system::alert("Some items couldn't be moved", &errors.join("\n"));
                    }
                }
            }
            Deferred::Share(files) => {
                let owner = with_controller(|c| c.main).unwrap_or_default();
                if let Err(e) = system::share(owner, &files) {
                    system::alert("Share", &e);
                }
            }
        }
    }
}

impl Controller {
    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(&self.settings_path) {
            self.deferred.push(Deferred::Alert("Couldn't save settings".into(), e.to_string()));
        }
    }

    fn items_for(&self, mode: WheelMode, files: &[PathBuf]) -> Vec<WheelItem> {
        let formats: Vec<_> = files.iter().map(|p| detect_format(p)).collect();
        let env = &self.env;
        wheel_items(mode, &formats, &|item| capability::item_available(env, item, &formats))
    }

    fn model(&self, mode: WheelMode, items: Vec<WheelItem>, keyboard: bool, files: &[PathBuf]) -> WheelModel {
        let selected = if keyboard && !items.is_empty() { Some(0) } else { None };
        let bytes: Option<u64> = files
            .iter()
            .map(|f| std::fs::metadata(f).ok().filter(|m| m.is_file()).map(|m| m.len()))
            .sum();
        WheelModel {
            mode,
            items,
            selected,
            keyboard,
            feedback: self.settings.feedback,
            combos: self.settings.wheels,
            hub: satsuma_core::wheel::hub_caption(files.len(), bytes),
            update: self.update_ready.clone(),
            estimates: Default::default(),
            estimating: false,
            phase: 0.0,
            liquid: Default::default(),
            on_glass_window: false,
        }
    }

    /// Starts working out output sizes for the wheel's petals.
    fn start_estimates(&mut self, files: &[PathBuf], items: &[WheelItem]) {
        let previewable = items.iter().any(|i| super::estimates::operation(i).is_some());
        self.wheel.set_estimating(previewable);
        self.estimates = None;
        if previewable {
            let formats: Vec<_> = files.iter().map(|p| detect_format(p)).collect();
            let ahead = satsuma_engine::estimate::is_cheap(files, &formats);
            self.estimates = Some(super::estimates::Session::start(self.env.clone(), self.settings.clone(), files.to_vec(), items.to_vec(), ahead));
        }
    }

    fn poll_estimates(&mut self) {
        if self.estimates.is_some() && !self.wheel.is_open() {
            self.estimates = None;
        }
        let selected = self.wheel.selected_item();
        let Some(session) = self.estimates.as_mut() else { return };
        session.prioritize(selected);
        for (item, estimate) in session.poll() {
            self.wheel.set_estimate(item, estimate);
        }
    }

    // ------------------------------------------------------------ the 60 Hz tick

    fn tick(&mut self) {
        let ready = super::update::ready();
        if ready != self.update_ready {
            debug!("update ready: {ready:?}");
            let fresh = self.update_ready.is_none();
            self.update_ready = ready;
            let tip = self.tip();
            self.tray.update(&tip);
            if let (true, Some(v)) = (fresh, &self.update_ready) {
                // Downloaded just now: say so once.
                self.tray.notify(&format!("Satsuma {v} is ready"), "It installs the next time Satsuma starts. Click here to install it now.");
            }
        }
        self.sample_drag();
        for event in events::drain() {
            self.handle(event);
        }
        self.poll_job();
        self.poll_estimates();
        self.wheel.animate();
        if self.wheel_hide_at.is_some_and(|t| Instant::now() >= t) {
            self.wheel_hide_at = None;
            self.wheel.hide();
        }
        if let Some(at) = self.hud_hide_at {
            if Instant::now() >= at {
                self.hud.hide();
                self.hud_hide_at = None;
            }
        }
        // Idle (no button held, nothing on screen or running), 20 checks a second are enough
        // to catch the start of a drag; keep the full frame rate only while it matters.
        let busy = system::left_mouse_down()
            || !matches!(self.drag, Drag::Idle)
            || self.wheel.is_visible()
            || self.keyboard_files.is_some()
            || self.hud_hide_at.is_some()
            || self.wheel_hide_at.is_some()
            || self.job.is_some()
            || !self.queue.is_empty()
            || self.estimates.is_some();
        let ms = if busy { FAST_TICK_MS } else { IDLE_TICK_MS };
        if ms != self.tick_ms {
            self.tick_ms = ms;
            unsafe { SetTimer(Some(self.main), TIMER_TICK, ms, None) };
            if !busy {
                // The wheel's and HUD's frames aren't needed until the next gesture.
                super::gfx::Surface::release_pool();
            }
        }
    }

    /// The wheel the keys ask for. An exact match wins; otherwise a key pressed on top of
    /// others (Shift or Ctrl held to make Explorer move or copy, say) decides by itself.
    fn wanted_mode(&self, mods: Mods, prev: Mods, changed: bool) -> Option<WheelMode> {
        let combos = &self.settings.wheels;
        if let Some(m) = combos.mode_for(mods) {
            return Some(m);
        }
        if !changed {
            return None;
        }
        combos.mode_for(Mods::from_bits(mods.bits() & !prev.bits()))
    }

    fn sample_drag(&mut self) {
        let mouse_down = system::left_mouse_down();
        let mods = system::current_mods();
        let at = system::cursor_pos();
        let prev = self.prev_mods;
        let changed = mods != prev;
        self.prev_mods = mods;
        if changed {
            debug!("keys {:?} (button {})", mods, if mouse_down { "down" } else { "up" });
        }
        if self.keyboard_files.is_some() {
            // The menu-opened wheel owns the window; ignore drag gestures meanwhile.
            return;
        }

        if !mouse_down {
            self.press = None;
            self.probe_from = None;
            self.probes = 0;
            match std::mem::replace(&mut self.drag, Drag::Idle) {
                Drag::Open { .. } => {
                    debug!("gesture close");
                    if self.wheel.has_selection() {
                        self.wheel_hide_at = Some(Instant::now() + Duration::from_millis(400));
                    } else {
                        self.wheel.hide();
                    }
                }
                Drag::Probing { .. } => self.wheel.hide(),
                _ => {}
            }
            return;
        }

        let press = *self.press.get_or_insert(at);
        let wanted = self.wanted_mode(mods, prev, changed);
        match &mut self.drag {
            Drag::Idle => {
                // A key tapped mid-drag, or one held while the pointer travels (the drag may
                // only start, or reach a window, after the key went down).
                let from = self.probe_from.unwrap_or(press);
                let moved = (at.0 - from.0).abs().max((at.1 - from.1).abs()) > unsafe { GetSystemMetrics(SM_CXDRAG) } * 2;
                if let Some(mode) = wanted.filter(|_| (changed || moved) && self.probes < MAX_PROBES) {
                    debug!("probe for {mode:?} at {at:?}");
                    self.probes += 1;
                    self.probe_from = Some(at);
                    self.wheel_hide_at = None;
                    self.wheel.probe(at);
                    system::nudge_pointer();
                    self.drag = Drag::Probing { mode, since: Instant::now() };
                }
            }
            Drag::Probing { mode, since } => {
                if let Some(m) = wanted.filter(|_| changed) {
                    *mode = m;
                }
                if since.elapsed() > PROBE_TIMEOUT {
                    // No drag reached the probe (yet): try again once the pointer moves on.
                    debug!("probe timed out");
                    self.wheel.hide();
                    self.drag = Drag::Idle;
                }
            }
            Drag::Open { mode, files } => {
                if let Some(m) = wanted.filter(|m| changed && *m != *mode) {
                    debug!("gesture switch to {m:?}");
                    let files = files.clone();
                    *mode = m;
                    let items = self.items_for(m, &files);
                    self.wheel.switch(m, items.clone());
                    self.start_estimates(&files, &items);
                }
            }
            Drag::Rejected => {}
        }
    }

    fn handle(&mut self, event: UiEvent) {
        debug!("event {event:?}");
        match event {
            UiEvent::ProbeConfirmed { files } => {
                if let Drag::Probing { mode, .. } = self.drag {
                    let items = self.items_for(mode, &files);
                    let model = self.model(mode, items.clone(), false, &files);
                    self.wheel.confirm(model);
                    self.start_estimates(&files, &items);
                    self.drag = Drag::Open { mode, files };
                }
            }
            UiEvent::ProbeRejected => {
                if matches!(self.drag, Drag::Probing { .. }) {
                    self.wheel.hide();
                    self.drag = Drag::Rejected;
                }
            }
            UiEvent::WheelDrop { item, files, at } => {
                self.wheel_hide_at = None;
                self.wheel.hide();
                self.drag = Drag::Rejected;
                self.dispatch(item, files, at);
            }
            UiEvent::WheelPick { item } => {
                let at = self.wheel.anchor();
                self.wheel.hide();
                if let Some(files) = self.keyboard_files.take() {
                    self.dispatch(item, files, at);
                }
            }
            UiEvent::WheelCancel => {
                if self.keyboard_files.take().is_some() {
                    self.wheel.hide();
                }
            }
            UiEvent::WheelSwitch(mode) => {
                if let Some(files) = self.keyboard_files.clone() {
                    let items = self.items_for(mode, &files);
                    self.wheel.switch(mode, items.clone());
                    self.start_estimates(&files, &items);
                }
            }
            UiEvent::HudClose => match &self.job {
                Some(job) => job.cancel.cancel(),
                None => {
                    self.hud.hide();
                    self.hud_hide_at = None;
                }
            },
            UiEvent::WindowClosed { window } => {
                self.floats.retain(|f| f.id() != window);
                self.editors.retain(|e| e.id() != window);
            }
            UiEvent::EditorApply { op, files, at } => self.enqueue(PendingJob { op, files, at }),
            UiEvent::RestartToUpdate => self.command(CMD_UPDATE),
            UiEvent::SettingsEdited(edited) => {
                let welcome = self.settings.has_completed_welcome;
                self.settings = *edited;
                self.settings.has_completed_welcome = welcome;
                self.save_settings();
                let tip = self.tip();
                self.tray.update(&tip);
                super::editor::refresh_all();
            }
            UiEvent::WindowDeactivated { window } => {
                if self.keyboard_files.is_some() && self.wheel.hwnd.0 as isize == window {
                    self.keyboard_files = None;
                    self.wheel.hide();
                }
            }
        }
    }

    fn flash(&mut self, at: Point, title: &str, subtitle: &str) {
        self.hud.show(at, HudModel { title: title.into(), subtitle: subtitle.into(), fraction: None, error: false });
        self.hud_hide_at = Some(Instant::now() + Duration::from_millis(1400));
    }

    fn dispatch(&mut self, item: WheelItem, files: Vec<PathBuf>, at: Point) {
        match item {
            WheelItem::Convert(f) => self.enqueue(PendingJob { op: Operation::Convert(f), files, at }),
            WheelItem::Tool(t) if t.has_editor() => self.open_editor(t, files),
            WheelItem::Tool(t) => self.enqueue(PendingJob { op: Operation::Tool(t), files, at }),
            WheelItem::Action(action) => self.perform_action(action, files, at),
        }
    }

    fn open_editor(&mut self, tool: satsuma_core::Tool, files: Vec<PathBuf>) {
        match EditorWindow::open(tool, files) {
            Ok(e) => self.editors.push(e),
            Err(e) => self.deferred.push(Deferred::Alert(format!("Couldn't open {}", tool.info().name), e)),
        }
    }

    fn perform_action(&mut self, action: Action, files: Vec<PathBuf>, at: Point) {
        let n = files.len();
        let noun = if n == 1 { "item".to_string() } else { format!("{n} items") };
        match action {
            Action::Copy => {
                if system::copy_files(&files) {
                    self.flash(at, "Copied", &format!("{noun} ready to paste in File Explorer"));
                } else {
                    self.flash(at, "Couldn't copy", "Another app is using the clipboard. Try again.");
                }
            }
            Action::CopyPath => {
                if system::copy_paths(&files) {
                    self.flash(at, "Copied Path", &if n == 1 { files[0].display().to_string() } else { format!("{n} paths, one per line") });
                } else {
                    self.flash(at, "Couldn't copy", "Another app is using the clipboard. Try again.");
                }
            }
            Action::Terminal => {
                if let Err(e) = system::open_terminal(&files[0], self.settings.terminal) {
                    self.deferred.push(Deferred::Alert("Terminal".into(), e));
                }
            }
            Action::Move => self.deferred.push(Deferred::Move(files)),
            Action::AirDrop => self.deferred.push(Deferred::Share(files)),
            Action::Float => {
                let existing = self.settings.float_behavior == FloatBehavior::AddToExisting && !self.floats.is_empty();
                if existing {
                    let f = self.floats.last().unwrap();
                    f.add(&files);
                    f.bring_to_front();
                } else {
                    let f = FloatWindow::new(&files, at);
                    self.floats.push(f);
                }
            }
        }
    }

    // ------------------------------------------------------------ jobs

    fn enqueue(&mut self, job: PendingJob) {
        self.queue.push(job);
        if self.job.is_none() {
            self.start_next();
        }
    }

    fn start_next(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        let PendingJob { op, files, at } = self.queue.remove(0);
        let (tx, rx) = channel();
        let cancel = Cancel::new();
        let env = self.env.clone();
        let settings = self.settings.clone();
        let job_cancel = cancel.clone();
        let title = op.title();
        std::thread::spawn(move || {
            satsuma_engine::win::ensure_com();
            let p = match plan(&env, &files, op) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(JobMsg::Failed(e.to_string()));
                    return;
                }
            };
            let ctx = Context { env: &env, settings: &settings, cancel: &job_cancel };
            let progress_tx = tx.clone();
            let report = run_plan(&ctx, &p, &mut |bp| {
                let _ = progress_tx.send(JobMsg::Progress(bp));
            });
            let _ = tx.send(JobMsg::Done { report, skipped: p.skipped });
        });
        self.hud.show(at, HudModel { title: title.clone(), subtitle: "Preparing…".into(), fraction: Some(0.0), error: false });
        self.hud_hide_at = None;
        self.job = Some(RunningJob { cancel, rx, title });
        self.set_working(true);
    }

    fn poll_job(&mut self) {
        let Some(job) = &self.job else { return };
        let mut finished: Option<(HudModel, Duration)> = None;
        let mut latest: Option<BatchProgress> = None;
        loop {
            match job.rx.try_recv() {
                Ok(JobMsg::Progress(p)) => latest = Some(p),
                Ok(JobMsg::Done { report, skipped }) => {
                    finished = Some(summarize(&job.title, &report, &skipped));
                    break;
                }
                Ok(JobMsg::Failed(msg)) => {
                    finished = Some((HudModel { title: "Couldn't start".into(), subtitle: msg, fraction: None, error: true }, Duration::from_secs(6)));
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = Some((
                        HudModel { title: "Operation failed".into(), subtitle: "The worker stopped unexpectedly.".into(), fraction: None, error: true },
                        Duration::from_secs(6),
                    ));
                    break;
                }
            }
        }
        if let Some(p) = latest {
            let title = job.title.clone();
            self.hud.update(HudModel { title, subtitle: format!("{} of {} saved", p.done, p.total), fraction: Some(p.fraction), error: false });
        }
        if let Some((model, linger)) = finished {
            debug!("job finished: {} — {}", model.title, model.subtitle);
            self.job = None;
            self.hud.update(model);
            self.hud_hide_at = Some(Instant::now() + linger);
            self.set_working(false);
            self.start_next();
            if self.exit_when_idle && self.job.is_none() {
                unsafe { PostQuitMessage(0) };
            }
        }
    }

    fn set_working(&self, working: bool) {
        let tip = if working { "Satsuma is working…".to_string() } else { self.tip() };
        self.tray.update(&tip);
    }

    fn tip(&self) -> String {
        let w = &self.settings.wheels;
        let tip = format!("Satsuma — drag files and tap {}, {} or {}", w.conversions.label(), w.tools.label(), w.actions.label());
        match &self.update_ready {
            Some(v) => format!("{tip}\nVersion {v} is ready; it installs when Satsuma restarts."),
            None => tip,
        }
    }

    // ------------------------------------------------------------ menu

    fn open_keyboard_wheel(&mut self, files: Vec<PathBuf>) {
        if files.is_empty() {
            return;
        }
        let has_conversions = !self.items_for(WheelMode::Conversions, &files).is_empty();
        let mode = initial_keyboard_mode(has_conversions);
        let items = self.items_for(mode, &files);
        let model = self.model(mode, items.clone(), true, &files);
        let (x, y) = system::cursor_pos();
        let work = system::work_area_at(x, y);
        self.wheel.show(model, ((work.left + work.right) / 2, (work.top + work.bottom) / 2));
        self.start_estimates(&files, &items);
        self.keyboard_files = Some(files);
    }

    fn command(&mut self, id: u32) {
        match id {
            CMD_HELP => self.deferred.push(Deferred::Welcome),
            CMD_QUIT => unsafe { PostQuitMessage(0) },
            CMD_SETTINGS => super::settings_window::open(&self.settings),
            CMD_UPDATE => {
                if self.job.is_some() {
                    self.deferred.push(Deferred::Alert("Satsuma is working".into(), "Restart to update once the current files are done.".into()));
                    return;
                }
                match super::update::install() {
                    Ok(version) => {
                        debug!("installed {version}; restarting");
                        if super::update::relaunch() {
                            unsafe { PostQuitMessage(0) };
                        }
                    }
                    Err(e) => self.deferred.push(Deferred::Alert("Couldn't update Satsuma".into(), e)),
                }
            }
            _ => {}
        }
    }
}

fn summarize(title: &str, report: &BatchReport, skipped: &[(PathBuf, String)]) -> (HudModel, Duration) {
    let saved = report.outputs.len();
    if report.cancelled {
        return (HudModel { title: "Cancelled".into(), subtitle: format!("{saved} saved before cancelling"), fraction: None, error: false }, Duration::from_millis(1200));
    }
    if let Some((path, err)) = report.errors.first() {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let more = if report.errors.len() > 1 { format!(" (+{} more)", report.errors.len() - 1) } else { String::new() };
        return (HudModel { title: "Finished with errors".into(), subtitle: format!("{name}: {err}{more}"), fraction: None, error: true }, Duration::from_secs(8));
    }
    if saved == 0 {
        let reason = skipped.first().map(|(_, r)| r.clone()).unwrap_or_else(|| "Nothing to do.".into());
        return (HudModel { title: "Nothing to do".into(), subtitle: reason, fraction: None, error: true }, Duration::from_secs(5));
    }
    let subtitle = if saved == 1 {
        format!("Saved {}", report.outputs[0].file_name().unwrap_or_default().to_string_lossy())
    } else {
        format!("{saved} of {saved} saved")
    };
    (HudModel { title: title.into(), subtitle, fraction: Some(1.0), error: false }, Duration::from_millis(1600))
}

// ---------------------------------------------------------------- menu building

struct Menu(HMENU);

impl Menu {
    fn new() -> Menu {
        Menu(unsafe { CreatePopupMenu().unwrap_or_default() })
    }

    fn item(&self, id: u32, title: &str, checked: bool, enabled: bool) {
        let mut flags = MF_STRING;
        if checked {
            flags |= MF_CHECKED;
        }
        if !enabled {
            flags |= MF_GRAYED;
        }
        unsafe {
            let _ = AppendMenuW(self.0, flags, id as usize, &HSTRING::from(title));
        }
    }

    fn separator(&self) {
        unsafe {
            let _ = AppendMenuW(self.0, MF_SEPARATOR, 0, None);
        }
    }

}

fn build_menu(working: bool, update: Option<&str>) -> Menu {
    let menu = Menu::new();
    menu.item(0, if working { "Satsuma is working…" } else { "Satsuma is ready" }, false, false);
    if let Some(v) = update {
        menu.item(CMD_UPDATE, &format!("Restart to Update ({v})"), false, true);
    }
    menu.separator();
    menu.item(CMD_CONVERT, "Convert Files…", false, true);
    menu.item(CMD_SETTINGS, "Settings…", false, true);
    menu.separator();
    menu.item(CMD_HELP, "How to Use Satsuma", false, true);
    menu.item(CMD_QUIT, "Quit Satsuma", false, true);
    menu
}

/// Shows the tray menu and runs the chosen command (outside the controller borrow while open).
fn show_menu(hwnd: HWND) {
    let Some((working, update)) = with_controller(|c| (c.job.is_some(), c.update_ready.clone())) else { return };
    let menu = build_menu(working, update.as_deref());
    let mut pt = POINT::default();
    let cmd = unsafe {
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenuEx(menu.0, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN).0, pt.x, pt.y, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu.0);
        cmd.0 as u32
    };
    if cmd == CMD_CONVERT {
        let files = system::choose_files();
        with_controller(|c| c.open_keyboard_wheel(files));
    } else if cmd != 0 {
        with_controller(|c| c.command(cmd));
    }
    run_deferred();
}

// ---------------------------------------------------------------- main window

struct Main {
    taskbar_created: u32,
}

impl Handler for Main {
    fn message(&self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_TIMER if wparam.0 == TIMER_TICK => {
                with_controller(|c| c.tick());
                run_deferred();
                Some(LRESULT(0))
            }
            WM_TRAY => {
                let event = window::loword(lparam.0) as u32;
                if event == WM_CONTEXTMENU || event == NIN_SELECT {
                    show_menu(hwnd);
                } else if event == NIN_BALLOONUSERCLICK {
                    // The "update is ready" notification: install now.
                    with_controller(|c| {
                        if c.update_ready.is_some() {
                            c.command(CMD_UPDATE);
                        }
                    });
                    run_deferred();
                }
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                // Light/dark switches: recolour the tray icon.
                with_controller(|c| {
                    // Swap in an icon for the new theme and free the old one.
                    let old = std::mem::replace(&mut c.tray.icon, tray_icon_for_theme());
                    if !old.is_invalid() {
                        unsafe {
                            let _ = DestroyIcon(old);
                        }
                    }
                    let tip = c.tip();
                    c.tray.update(&tip);
                });
                None
            }
            m if m == self.taskbar_created && m != 0 => {
                // Explorer restarted: put the icon back.
                with_controller(|c| {
                    let tip = c.tip();
                    c.tray.add(&tip);
                });
                Some(LRESULT(0))
            }
            WM_DESTROY => {
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

/// Debug hook: `SATSUMA_SELFTEST_DROP="convert:mp3|C:\a.wav|C:\b.wav"` (or `tool:<id>|…`)
/// posts the same event a real drop on a petal posts, then quits when the job is done.
#[cfg(debug_assertions)]
fn selftest_drop() -> Option<UiEvent> {
    let spec = std::env::var("SATSUMA_SELFTEST_DROP").ok()?;
    let mut parts = spec.split('|');
    let (kind, id) = parts.next()?.split_once(':')?;
    let item = match kind {
        "convert" => WheelItem::Convert(satsuma_core::Format::from_id(id)?),
        "tool" => WheelItem::Tool(satsuma_core::Tool::from_id(id)?),
        "action" => WheelItem::Action(Action::from_id(id)?),
        _ => return None,
    };
    let files = parts.map(PathBuf::from).collect();
    Some(UiEvent::WheelDrop { item, files, at: system::cursor_pos() })
}

/// Only one Satsuma per session: a second launch shows a note and quits.
fn already_running() -> bool {
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    unsafe {
        // Just after an update the old instance is still quitting: wait for it.
        let tries = if super::update::after_update() { 50 } else { 1 };
        for k in 0..tries {
            let mutex = CreateMutexW(None, true, &HSTRING::from("Local\\Satsuma.SingleInstance"));
            let exists = GetLastError() == ERROR_ALREADY_EXISTS;
            if !exists {
                std::mem::forget(mutex);
                return false;
            }
            if let Ok(h) = mutex {
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
            if k + 1 < tries {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        true
    }
}

pub fn run() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = OleInitialize(None);
    }
    let selftest = std::env::var_os("SATSUMA_SELFTEST_DROP").is_some();
    super::update::clean_up();
    if !selftest && !super::update::after_update() {
        // An update downloaded last time goes in before anything starts.
        match super::update::install() {
            Ok(version) => {
                debug!("installed {version} at start");
                if super::update::relaunch() {
                    return;
                }
            }
            Err(e) => debug!("no update installed: {e}"),
        }
    }
    if already_running() && !selftest {
        system::alert("Satsuma", "Satsuma is already running. Its icon is in the notification area, at the right end of the taskbar.");
        return;
    }
    // After an update the old instance has quit by now, so its executables can go.
    super::update::clean_up();
    let taskbar_created = unsafe { RegisterWindowMessageW(&HSTRING::from("TaskbarCreated")) };
    let main = window::create(WS_EX_TOOLWINDOW, WS_OVERLAPPED, "Satsuma", 0, 0, 0, 0, None, Rc::new(Main { taskbar_created }));

    let settings_path = Settings::default_path();
    let settings = Settings::load(&settings_path);
    let welcome = !settings.has_completed_welcome;
    super::theme::set_appearance(settings.appearance);
    super::theme::set_transparency(settings.transparency);
    let tray = Tray { hwnd: main, icon: tray_icon_for_theme() };
    system::set_clipboard_owner(main);
    let mut controller = Controller {
        settings,
        settings_path,
        env: Arc::new(Env::discover()),
        main,
        tray,
        wheel: WheelWindow::new(),
        keyboard_files: None,
        drag: Drag::Idle,
        prev_mods: system::current_mods(),
        press: None,
        probe_from: None,
        probes: 0,
        hud: Hud::new(),
        hud_hide_at: None,
        wheel_hide_at: None,
        floats: Vec::new(),
        editors: Vec::new(),
        estimates: None,
        job: None,
        queue: Vec::new(),
        deferred: Vec::new(),
        exit_when_idle: false,
        tick_ms: FAST_TICK_MS,
        update_ready: None,
    };
    #[cfg(debug_assertions)]
    if let Some(event) = selftest_drop() {
        controller.exit_when_idle = true;
        events::post(event);
    }
    controller.update_ready = super::update::ready();
    let tip = controller.tip();
    controller.tray.add(&tip);
    super::update::start();
    if super::update::after_update() {
        let at = system::cursor_pos();
        controller.flash(at, "Satsuma updated", &format!("Now on version {}", super::update::CURRENT));
        controller.hud_hide_at = Some(Instant::now() + Duration::from_secs(4));
    }
    #[cfg(debug_assertions)]
    if std::env::var_os("SATSUMA_OPEN_SETTINGS").is_some() {
        super::settings_window::open(&controller.settings);
    }
    if welcome {
        controller.settings.has_completed_welcome = true;
        controller.save_settings();
        controller.deferred.push(Deferred::Welcome);
    }
    if !controller.env.has_ffmpeg() {
        controller.deferred.push(Deferred::Alert(
            "Audio and video need ffmpeg".into(),
            "Install it with `winget install Gyan.FFmpeg` in a terminal, then restart Satsuma. Images, documents, PDFs, spreadsheets, subtitles and archives work without it.".into(),
        ));
    }
    debug!("app started; settings {}; ffmpeg {:?}; browser {:?}", controller.settings_path.display(), controller.env.ffmpeg, controller.env.browser);
    CONTROLLER.with(|c| *c.borrow_mut() = Some(controller));

    unsafe {
        SetTimer(Some(main), TIMER_TICK, FAST_TICK_MS, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if super::editor::translate_dialog_message(&msg) {
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    CONTROLLER.with(|c| {
        if let Some(c) = c.borrow_mut().take() {
            if let Some(job) = &c.job {
                job.cancel.cancel();
            }
            c.tray.remove();
        }
    });
}
