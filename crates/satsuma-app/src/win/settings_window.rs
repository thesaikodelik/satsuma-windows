//! The Settings window: every preference in one place, in four tabs (Appearance, Wheel,
//! Files, General). Changes apply and save as they are made.

use super::editor::{apply_theme, calm, goo, spring, PILL_SECONDS};
use super::events::{self, UiEvent};
use super::gfx::{Align, Painter, Rgba, Surface, Weight};
use super::theme::{Palette, ACCENT};
use super::wheel::{self, WheelModel};
use super::window::{self, Handler};
use super::system;
use satsuma_core::geometry::{WheelGeometry, WHEEL_VIEW_SIZE};
use satsuma_core::settings::{Appearance, FloatBehavior, Settings, Strength, TerminalApp};
use satsuma_core::{Format, Mods, WheelItem, WheelMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, ClientToScreen, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const TITLE: &str = "Satsuma Settings";

/// Content size in DIPs.
const W: f32 = 460.0;
const H: f32 = 520.0;
const PILL_TIMER: usize = 3;

/// Wheel key choices (the Windows key is left out: tapping it opens Start).
pub const COMBO_CHOICES: [Mods; 6] = [
    Mods::SHIFT,
    Mods::CONTROL,
    Mods::OPTION,
    Mods::CONTROL.union(Mods::SHIFT),
    Mods::OPTION.union(Mods::SHIFT),
    Mods::CONTROL.union(Mods::OPTION),
];
const IMAGE_SIZES: [u32; 4] = [0, 4096, 2048, 1024];
const TABS: [&str; 4] = ["General", "Appearance", "Shortcuts", "Files"];
const GENERAL: usize = 0;
const APPEARANCE: usize = 1;
const SHORTCUTS: usize = 2;
const FILES: usize = 3;

type R = (f32, f32, f32, f32);

fn contains(r: R, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

/// A segmented control.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Group {
    Tabs,
    Theme,
    /// Compression strength: 0 images, 1 video, 2 audio.
    Strength(u8),
}

/// A field that picks from a short list.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Picker {
    Key(usize),
    ImageSize,
    Split,
    Terminal,
    Float,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Toggle {
    Sound,
    Login,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Id {
    Segment(Group, usize),
    Transparency,
    Picker(Picker),
    Toggle(Toggle),
    Done,
}

struct State {
    settings: Settings,
    login: bool,
    tab: usize,
    /// A segmented control's pill flowing from one choice to another.
    pill: Option<(Group, usize, Instant)>,
    pressed: Option<Id>,
    dragging: bool,
    preview: WheelModel,
    scale: f32,
}

/// Rows hold a label on the left and a control on the right.
const ROW_LABEL_X: f32 = 20.0;
const CONTROL_W: f32 = 220.0;
const CONTROL_X: f32 = W - 20.0 - CONTROL_W;
const PREVIEW_Y: f32 = 54.0;
/// The sound switch's row on the Shortcuts tab.
const SOUND_Y: f32 = 418.0;

fn row(y: f32) -> R {
    (CONTROL_X, y, CONTROL_W, 30.0)
}

impl State {
    fn segments(&self, g: Group) -> (R, Vec<&'static str>, usize) {
        match g {
            Group::Tabs => ((20.0, 12.0, W - 40.0, 30.0), TABS.to_vec(), self.tab),
            Group::Theme => (
                row(PREVIEW_Y + WHEEL_VIEW_SIZE as f32 + 8.0),
                Appearance::ALL.iter().map(|a| a.name()).collect(),
                Appearance::ALL.iter().position(|a| *a == self.settings.appearance).unwrap_or(0),
            ),
            Group::Strength(k) => {
                let current = match k {
                    0 => self.settings.image_compression,
                    1 => self.settings.video_compression,
                    _ => self.settings.audio_compression,
                };
                (row(96.0 + k as f32 * 42.0), vec!["Balanced", "Strong"], (current == Strength::Strong) as usize)
            }
        }
    }

    fn groups(&self) -> Vec<Group> {
        match self.tab {
            APPEARANCE => vec![Group::Tabs, Group::Theme],
            FILES => vec![Group::Tabs, Group::Strength(0), Group::Strength(1), Group::Strength(2)],
            _ => vec![Group::Tabs],
        }
    }

    fn widgets(&self) -> Vec<(Id, R)> {
        let mut v = Vec::new();
        for g in self.groups() {
            let (r, names, _) = self.segments(g);
            let n = names.len() as f32;
            for i in 0..names.len() {
                v.push((Id::Segment(g, i), (r.0 + r.2 * i as f32 / n, r.1, r.2 / n, r.3)));
            }
        }
        match self.tab {
            APPEARANCE => v.push((Id::Transparency, (CONTROL_X - 30.0, PREVIEW_Y + WHEEL_VIEW_SIZE as f32 + 50.0, CONTROL_W - 20.0, 26.0))),
            SHORTCUTS => {
                for k in 0..3 {
                    v.push((Id::Picker(Picker::Key(k)), row(84.0 + k as f32 * 42.0)));
                }
                v.push((Id::Toggle(Toggle::Sound), (W - 20.0 - 44.0, SOUND_Y + 3.0, 44.0, 24.0)));
            }
            FILES => {
                v.push((Id::Picker(Picker::ImageSize), row(222.0)));
                v.push((Id::Picker(Picker::Split), row(298.0)));
            }
            _ => {
                v.push((Id::Picker(Picker::Terminal), row(70.0)));
                v.push((Id::Picker(Picker::Float), row(112.0)));
                v.push((Id::Toggle(Toggle::Login), (W - 20.0 - 44.0, 160.0, 44.0, 24.0)));
            }
        }
        v.push((Id::Done, (W - 20.0 - 110.0, H - 46.0, 110.0, 30.0)));
        v
    }

    fn hit(&self, x: f32, y: f32) -> Option<Id> {
        self.widgets().into_iter().find(|(_, r)| contains(*r, x, y)).map(|(id, _)| id)
    }

    fn rect_of(&self, id: Id) -> Option<R> {
        self.widgets().into_iter().find(|(i, _)| *i == id).map(|(_, r)| r)
    }

    fn moving(&self) -> bool {
        self.pill.is_some_and(|(_, _, t)| t.elapsed().as_secs_f32() < PILL_SECONDS) || self.preview.liquid.moving()
    }

    /// Applies the settings everywhere at once and tells the app to save them.
    fn changed(&self) {
        super::theme::set_appearance(self.settings.appearance);
        super::theme::set_transparency(self.settings.transparency);
        events::post(UiEvent::SettingsEdited(Box::new(self.settings.clone())));
    }

    fn select(&mut self, g: Group, i: usize) {
        let (_, _, from) = self.segments(g);
        if from != i {
            self.pill = Some((g, from, Instant::now()));
        }
        match g {
            Group::Tabs => {
                self.tab = i;
                return;
            }
            Group::Theme => self.settings.appearance = Appearance::ALL[i.min(Appearance::ALL.len() - 1)],
            Group::Strength(k) => {
                let s = if i == 1 { Strength::Strong } else { Strength::Balanced };
                match k {
                    0 => self.settings.image_compression = s,
                    1 => self.settings.video_compression = s,
                    _ => self.settings.audio_compression = s,
                }
            }
        }
        self.changed();
    }

    fn set_transparency_from_x(&mut self, x: f32) {
        let Some(r) = self.rect_of(Id::Transparency) else { return };
        let t = ((x - r.0 - 8.0) / (r.2 - 16.0)).clamp(0.0, 1.0);
        let v = (t * 100.0).round() as u8;
        if v != self.settings.transparency {
            self.settings.transparency = v;
            self.changed();
        }
    }

    /// The choices of a picker and the current one.
    fn choices(&self, p: Picker) -> (Vec<String>, usize) {
        let s = &self.settings;
        match p {
            Picker::Key(k) => {
                let current = s.wheels.combo(WheelMode::ALL[k]);
                (COMBO_CHOICES.iter().map(|m| m.label()).collect(), COMBO_CHOICES.iter().position(|m| *m == current).unwrap_or(0))
            }
            Picker::ImageSize => (
                IMAGE_SIZES.iter().map(|px| if *px == 0 { "Keep original size".to_string() } else { format!("Longest edge {px} px") }).collect(),
                IMAGE_SIZES.iter().position(|px| *px == s.image_max_edge.unwrap_or(0)).unwrap_or(0),
            ),
            Picker::Split => ((2..=6).map(|n| format!("{n} equal parts")).collect(), (s.split_video_parts.clamp(2, 6) - 2) as usize),
            Picker::Terminal => (
                TerminalApp::choices().iter().map(|t| t.name().to_string()).collect(),
                TerminalApp::choices().iter().position(|t| *t == s.terminal).unwrap_or(0),
            ),
            Picker::Float => (
                vec!["Add to the open Float".into(), "Open a new Float".into()],
                (s.float_behavior == FloatBehavior::CreateNew) as usize,
            ),
        }
    }

    fn pick(&mut self, p: Picker, i: usize) {
        let s = &mut self.settings;
        match p {
            Picker::Key(k) => {
                let (mode, choice) = (WheelMode::ALL[k], COMBO_CHOICES[i.min(COMBO_CHOICES.len() - 1)]);
                // Swap with whichever wheel already uses the keys, so each stays unique.
                let old = s.wheels.combo(mode);
                for other in WheelMode::ALL {
                    if other != mode && s.wheels.combo(other) == choice {
                        s.wheels.set(other, old);
                    }
                }
                s.wheels.set(mode, choice);
            }
            Picker::ImageSize => {
                let px = IMAGE_SIZES[i.min(IMAGE_SIZES.len() - 1)];
                s.image_max_edge = (px != 0).then_some(px);
            }
            Picker::Split => s.split_video_parts = (i as u32 + 2).min(6),
            Picker::Terminal => {
                if let Some(t) = TerminalApp::choices().get(i) {
                    s.terminal = *t;
                }
            }
            Picker::Float => s.float_behavior = if i == 1 { FloatBehavior::CreateNew } else { FloatBehavior::AddToExisting },
        }
        self.changed();
    }

    fn toggle(&mut self, t: Toggle) {
        match t {
            Toggle::Sound => {
                self.settings.feedback = !self.settings.feedback;
                self.changed();
            }
            Toggle::Login => match system::set_launch_at_login(!self.login) {
                Ok(()) => self.login = system::launch_at_login(),
                Err(e) => system::alert("Couldn't update Launch at Login", &e),
            },
        }
    }

    // ------------------------------------------------------------------ painting

    fn paint(&self, p: &Painter, pal: &Palette) {
        p.fill_round_rect(0.0, 0.0, W, H, 0.0, if pal.glass { pal.backdrop } else { Rgba(pal.panel.0, pal.panel.1, pal.panel.2, 1.0) });
        let label = |text: &str, y: f32| p.text(text, (ROW_LABEL_X, y, 200.0, 30.0), 12.5, Weight::Regular, pal.label, Align::Leading, false);
        let header = |text: &str, y: f32| p.text(text, (ROW_LABEL_X, y, W - 40.0, 22.0), 11.0, Weight::Semibold, pal.secondary, Align::Leading, false);
        let note = |text: &str, y: f32, h: f32| p.text(text, (ROW_LABEL_X, y, W - 40.0, h), 11.0, Weight::Regular, pal.tertiary, Align::Leading, true);
        for g in self.groups() {
            self.paint_segments(p, pal, g);
        }
        match self.tab {
            APPEARANCE => {
                let wheel_pal = Palette::for_appearance(self.settings.appearance, pal.dark);
                let x = (W - WHEEL_VIEW_SIZE as f32) / 2.0;
                p.translated(x, PREVIEW_Y, &|p| wheel::paint(p, &self.preview, &wheel_pal, None, self.scale));
                let y = PREVIEW_Y + WHEEL_VIEW_SIZE as f32 + 8.0;
                label("Theme", y);
                label("Transparency", y + 48.0);
                self.paint_slider(p, pal);
                p.text(&format!("{} %", self.settings.transparency), (W - 62.0, y + 48.0, 44.0, 30.0), 12.0, Weight::Regular, pal.secondary, Align::Leading, false);
                note("Hover the sample's petals to try the highlight.", H - 46.0, 30.0);
            }
            SHORTCUTS => {
                header("WHEEL KEYS", 56.0);
                for (k, name) in ["Convert", "Tools", "Actions"].iter().enumerate() {
                    label(name, 84.0 + k as f32 * 42.0);
                    self.paint_picker(p, pal, Picker::Key(k));
                }
                header("HOW IT WORKS", 214.0);
                self.paint_tutorial(p, pal, 240.0);
                note("Tapping another key switches wheels. Drop in the middle to cancel.", 384.0, 20.0);
                label("Sound when a petal lights up", SOUND_Y);
                self.paint_toggle(p, pal, Toggle::Sound, self.settings.feedback);
            }
            FILES => {
                header("COMPRESSION", 64.0);
                for (k, name) in ["Images", "Video", "Audio"].iter().enumerate() {
                    label(name, 96.0 + k as f32 * 42.0);
                }
                label("Image size", 222.0);
                self.paint_picker(p, pal, Picker::ImageSize);
                header("SPLIT VIDEO", 272.0);
                label("Split into", 298.0);
                self.paint_picker(p, pal, Picker::Split);
            }
            _ => {
                label("Terminal app", 70.0);
                self.paint_picker(p, pal, Picker::Terminal);
                label("Float", 112.0);
                self.paint_picker(p, pal, Picker::Float);
                label("Launch at login", 157.0);
                self.paint_toggle(p, pal, Toggle::Login, self.login);
                note("Settings are saved as you change them.", H - 46.0, 30.0);
            }
        }
        // Done
        if let Some(r) = self.rect_of(Id::Done) {
            let pressed = self.pressed == Some(Id::Done);
            let radius = if pal.glass { r.3 / 2.0 } else { 6.0 };
            p.fill_round_rect(r.0, r.1, r.2, r.3, radius, if pressed { Rgba(0.85, 0.36, 0.07, 1.0) } else { ACCENT });
            p.text("Done", r, 12.5, Weight::Semibold, Rgba::WHITE, Align::Center, false);
        }
    }

    /// Three line drawings: drag files, tap the key once, drop on a petal.
    fn paint_tutorial(&self, p: &Painter, pal: &Palette, top: f32) {
        let ink = pal.secondary;
        let faint = pal.tertiary;
        let panel_w = (W - 40.0 - 2.0 * 12.0) / 3.0;
        let key = self.settings.wheels.conversions.label();
        let captions = ["Drag files".to_string(), format!("Tap {key} once"), "Drop on a petal".to_string()];
        for (i, caption) in captions.iter().enumerate() {
            let x = 20.0 + i as f32 * (panel_w + 12.0);
            let (cx, cy) = (x + panel_w / 2.0, top + 54.0);
            // A soft card behind each step.
            p.fill_round_rect(x, top, panel_w, 136.0, 14.0, if pal.glass { pal.control } else { pal.separator.alpha(0.35) });
            if pal.glass {
                p.stroke_round_rect(x + 0.5, top + 0.5, panel_w - 1.0, 135.0, 13.5, pal.rim, 1.0);
            }
            match i {
                0 => {
                    // A file being carried off by the pointer, with a motion trail.
                    draw_document(p, cx - 26.0, cy - 30.0, 34.0, 44.0, ink);
                    for k in 0..4 {
                        p.fill_circle(cx - 4.0 + k as f32 * 7.0, cy + 18.0 + k as f32 * 3.0, 1.4, faint);
                    }
                    draw_document(p, cx + 6.0, cy - 6.0, 26.0, 33.0, ACCENT.alpha(0.9));
                    draw_cursor(p, cx + 22.0, cy + 18.0, ink, pal);
                }
                1 => {
                    // A key pressed once.
                    let (kw, kh) = (if key.len() > 5 { 82.0 } else { 64.0 }, 40.0);
                    p.stroke_round_rect(cx - kw / 2.0, cy - kh / 2.0 + 6.0, kw, kh, 9.0, faint, 1.2);
                    p.stroke_round_rect(cx - kw / 2.0, cy - kh / 2.0, kw, kh, 9.0, ink, 1.6);
                    p.text(&key, (cx - kw / 2.0, cy - kh / 2.0, kw, kh), 12.0, Weight::Semibold, pal.label, Align::Center, false);
                    // Tap marks above the key.
                    for (dx, len) in [(-16.0f32, 7.0f32), (0.0, 9.0), (16.0, 7.0)] {
                        let (x0, y0) = (cx + dx, cy - kh / 2.0 - 8.0);
                        let ang = dx.atan2(28.0);
                        p.line(x0 + ang.sin() * 2.0, y0, x0 + ang.sin() * (2.0 + len), y0 - len * ang.cos(), ACCENT, 1.8);
                    }
                    p.text("× 1", (cx - 30.0, cy + kh / 2.0 + 4.0, 60.0, 18.0), 11.0, Weight::Semibold, ACCENT, Align::Center, false);
                }
                _ => {
                    // A small wheel with one petal lit, and the pointer on it.
                    let (ro, ri) = (34.0, 13.0);
                    p.stroke_circle(cx, cy, ro, ink, 1.4);
                    p.stroke_circle(cx, cy, ri, ink, 1.2);
                    for k in 0..6 {
                        let a = std::f32::consts::PI / 2.0 + std::f32::consts::PI / 6.0 + k as f32 * std::f32::consts::PI / 3.0;
                        p.line(cx + ri * a.cos(), cy - ri * a.sin(), cx + ro * a.cos(), cy - ro * a.sin(), ink, 1.2);
                    }
                    // The lit petal (top right).
                    let g = satsuma_core::geometry::WheelGeometry { inner: ri as f64 + 1.5, outer: ro as f64 - 1.5, gap: 2.0, outer_corner: 4.0, inner_corner: 2.0, backdrop: ro as f64 };
                    if let Some(geo) = p.path(&g.petal_path(1, 6), cx, cy) {
                        p.fill_geometry(&geo, ACCENT.alpha(0.9));
                    }
                    draw_cursor(p, cx + 16.0, cy - 14.0, ink, pal);
                }
            }
            // The step number and its caption.
            p.fill_circle(x + 16.0, top + 16.0, 9.0, ACCENT);
            p.text(&(i + 1).to_string(), (x + 7.0, top + 7.0, 18.0, 18.0), 10.5, Weight::Bold, Rgba::WHITE, Align::Center, false);
            p.text(caption, (x + 4.0, top + 104.0, panel_w - 8.0, 22.0), 11.5, Weight::Semibold, pal.label, Align::Center, false);
        }
    }

    fn paint_segments(&self, p: &Painter, pal: &Palette, g: Group) {
        let (track, names, selected) = self.segments(g);
        let n = names.len();
        p.fill_round_rect(track.0, track.1, track.2, track.3, track.3 / 2.0, if pal.glass { pal.control } else { pal.separator.alpha(0.6) });
        if pal.glass {
            p.stroke_round_rect(track.0 + 0.5, track.1 + 0.5, track.2 - 1.0, track.3 - 1.0, track.3 / 2.0 - 0.5, pal.rim, 1.0);
        }
        let w = track.2 / n as f32;
        let slot = |i: usize| (track.0 + w * i as f32 + 3.0, track.1 + 3.0, w - 6.0, track.3 - 6.0);
        let elapsed = match self.pill {
            Some((pg, _, t)) if pg == g => t.elapsed().as_secs_f32(),
            _ => PILL_SECONDS,
        };
        let pieces = match self.pill {
            Some((pg, from, _)) if pg == g && elapsed < PILL_SECONDS && from < n => {
                goo(slot(from), slot(selected), if calm() { 1.0 } else { spring(elapsed) })
            }
            _ => vec![slot(selected)],
        };
        let (fill, rim) = if pal.glass {
            (if pal.dark { Rgba(1.0, 1.0, 1.0, 0.16) } else { Rgba(1.0, 1.0, 1.0, 0.75) }, pal.rim)
        } else {
            (Rgba(pal.control.0, pal.control.1, pal.control.2, 1.0), pal.separator)
        };
        p.liquid(track, &pieces, fill, rim, self.scale);
        for (i, name) in names.iter().enumerate() {
            let r = (track.0 + w * i as f32, track.1, w, track.3);
            let arrived = i == selected && elapsed > 0.14;
            p.text(name, r, 12.0, if arrived { Weight::Semibold } else { Weight::Regular }, pal.label, Align::Center, false);
        }
    }

    fn paint_slider(&self, p: &Painter, pal: &Palette) {
        let Some(r) = self.rect_of(Id::Transparency) else { return };
        let (x0, w) = (r.0 + 8.0, r.2 - 16.0);
        let cy = r.1 + r.3 / 2.0;
        let t = self.settings.transparency as f32 / 100.0;
        p.fill_round_rect(x0, cy - 2.0, w, 4.0, 2.0, pal.separator.alpha(1.6));
        p.fill_round_rect(x0, cy - 2.0, w * t, 4.0, 2.0, ACCENT);
        let kx = x0 + w * t;
        p.fill_circle(kx, cy, 8.0, Rgba(0.0, 0.0, 0.0, 0.12));
        p.fill_circle(kx, cy, 7.5, if pal.dark { Rgba::hex(0x4a4a4a, 1.0) } else { Rgba::WHITE });
        p.stroke_circle(kx, cy, 7.5, pal.separator, 0.5);
        p.fill_circle(kx, cy, if self.dragging { 4.0 } else { 3.0 }, ACCENT);
    }

    fn paint_picker(&self, p: &Painter, pal: &Palette, k: Picker) {
        let Some(r) = self.rect_of(Id::Picker(k)) else { return };
        let (names, current) = self.choices(k);
        let radius = r.3 / 2.0;
        let pressed = self.pressed == Some(Id::Picker(k));
        p.fill_round_rect(r.0, r.1, r.2, r.3, radius, if pressed { pal.separator } else if pal.glass { pal.control.alpha(1.6) } else { pal.control });
        p.stroke_round_rect(r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, radius - 0.5, if pal.glass { pal.rim } else { pal.separator }, 1.0);
        p.text(&names[current], (r.0 + 14.0, r.1, r.2 - 40.0, r.3), 12.0, Weight::Regular, pal.label, Align::Leading, false);
        p.icon(char::from_u32(0xE70D).unwrap(), r.0 + r.2 - 18.0, r.1 + r.3 / 2.0, 10.0, pal.secondary);
    }

    fn paint_toggle(&self, p: &Painter, pal: &Palette, t: Toggle, on: bool) {
        let Some(r) = self.rect_of(Id::Toggle(t)) else { return };
        let radius = r.3 / 2.0;
        if on {
            p.fill_round_rect(r.0, r.1, r.2, r.3, radius, ACCENT);
        } else {
            p.fill_round_rect(r.0, r.1, r.2, r.3, radius, pal.control.alpha(if pal.glass { 1.6 } else { 1.0 }));
            p.stroke_round_rect(r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, radius - 0.5, if pal.glass { pal.rim } else { pal.tertiary }, 1.0);
        }
        let knob_x = if on { r.0 + r.2 - radius } else { r.0 + radius };
        p.fill_circle(knob_x, r.1 + radius, radius - 4.0, if on { Rgba::WHITE } else { pal.secondary });
    }

    // ------------------------------------------------------------------ pointer

    fn pointer_down(&mut self, x: f32, y: f32) {
        match self.hit(x, y) {
            Some(Id::Segment(g, i)) => self.select(g, i),
            Some(Id::Transparency) => {
                self.dragging = true;
                self.set_transparency_from_x(x);
            }
            Some(id) => self.pressed = Some(id),
            None => {}
        }
    }

    fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        if self.dragging {
            self.set_transparency_from_x(x);
            return true;
        }
        if self.tab == APPEARANCE {
            let half = (WHEEL_VIEW_SIZE / 2.0) as f32;
            let (dx, dy) = (x - (W - WHEEL_VIEW_SIZE as f32) / 2.0 - half, y - PREVIEW_Y - half);
            let hit = WheelGeometry::default().hit_test(dx as f64, -dy as f64, self.preview.items.len());
            if hit.is_some() && hit != self.preview.selected {
                self.preview.selected = hit;
                self.preview.aim_liquid();
                return true;
            }
        }
        false
    }
}

/// A page with a folded corner, outlined.
fn draw_document(p: &Painter, x: f32, y: f32, w: f32, h: f32, ink: Rgba) {
    let fold = w * 0.32;
    p.line(x, y, x + w - fold, y, ink, 1.5);
    p.line(x + w - fold, y, x + w, y + fold, ink, 1.5);
    p.line(x + w, y + fold, x + w, y + h, ink, 1.5);
    p.line(x + w, y + h, x, y + h, ink, 1.5);
    p.line(x, y + h, x, y, ink, 1.5);
    p.line(x + w - fold, y, x + w - fold, y + fold, ink, 1.2);
    p.line(x + w - fold, y + fold, x + w, y + fold, ink, 1.2);
    for k in 0..3 {
        let ly = y + fold + 8.0 + k as f32 * 6.0;
        if ly < y + h - 4.0 {
            p.line(x + 6.0, ly, x + w - 6.0 - if k == 2 { w * 0.25 } else { 0.0 }, ly, ink.alpha(0.6), 1.1);
        }
    }
}

/// The arrow pointer with its tip at `(x, y)`.
fn draw_cursor(p: &Painter, x: f32, y: f32, ink: Rgba, pal: &Palette) {
    use satsuma_core::geometry::{PathCmd, Point};
    // Designed y-down; the painter's paths are y-up around (x, y).
    let pts = [(0.0, 0.0), (0.0, 17.0), (4.2, 13.2), (7.2, 19.5), (10.0, 18.3), (7.1, 12.2), (12.5, 12.2)];
    let mut cmds: Vec<PathCmd> = pts.iter().enumerate().map(|(i, (px, py))| {
        let pt = Point::new(*px, -*py);
        if i == 0 { PathCmd::MoveTo(pt) } else { PathCmd::LineTo(pt) }
    }).collect();
    cmds.push(PathCmd::Close);
    if let Some(geo) = p.path(&cmds, x, y) {
        p.fill_geometry(&geo, if pal.dark { Rgba::hex(0x1A1C22, 1.0) } else { Rgba::WHITE });
        p.stroke_geometry(&geo, ink, 1.3);
    }
}

// ---------------------------------------------------------------------- window

struct Messages {
    state: Rc<RefCell<State>>,
}

fn scale_of(hwnd: HWND) -> f32 {
    (unsafe { GetDpiForWindow(hwnd) }.max(96)) as f32 / 96.0
}

fn refresh(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn animate(hwnd: HWND, s: &State) {
    if s.moving() {
        unsafe { SetTimer(Some(hwnd), PILL_TIMER, 10, None) };
    }
}

/// Shows a picker's choices as a menu under it; returns the one chosen.
fn popup(hwnd: HWND, r: R, names: &[String], current: usize) -> Option<usize> {
    let s = scale_of(hwnd);
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for (i, name) in names.iter().enumerate() {
            let flags = if i == current { MF_STRING | MF_CHECKED } else { MF_STRING };
            let _ = AppendMenuW(menu, flags, i + 1, &HSTRING::from(name.as_str()));
        }
        let mut pt = POINT { x: (r.0 * s) as i32, y: ((r.1 + r.3 + 2.0) * s) as i32 };
        let _ = ClientToScreen(hwnd, &mut pt);
        let cmd = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN).0, pt.x, pt.y, hwnd, None);
        let _ = DestroyMenu(menu);
        (cmd.0 > 0).then(|| cmd.0 as usize - 1)
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
                    if let Ok(mut s) = self.state.try_borrow_mut() {
                        s.scale = scale_of(hwnd);
                    }
                    if let (Ok(surface), Ok(s)) = (Surface::new(r.right, r.bottom), self.state.try_borrow()) {
                        let pal = Palette::current();
                        surface.draw(scale_of(hwnd), |p| s.paint(p, &pal));
                        surface.blit(dc, 0, 0);
                    }
                    let _ = EndPaint(hwnd, &ps);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_LBUTTONDOWN => {
                let (x, y) = dip(lparam);
                let mut s = self.state.try_borrow_mut().ok()?;
                s.pointer_down(x, y);
                unsafe { SetCapture(hwnd) };
                animate(hwnd, &s);
                refresh(hwnd);
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = dip(lparam);
                let mut s = self.state.try_borrow_mut().ok()?;
                if s.pointer_move(x, y) {
                    refresh(hwnd);
                }
                animate(hwnd, &s);
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                let (x, y) = dip(lparam);
                let clicked = {
                    let mut s = self.state.try_borrow_mut().ok()?;
                    s.dragging = false;
                    let pressed = s.pressed.take();
                    pressed.filter(|p| s.hit(x, y) == Some(*p))
                };
                refresh(hwnd);
                match clicked {
                    Some(Id::Done) => unsafe {
                        let _ = DestroyWindow(hwnd);
                    },
                    Some(Id::Toggle(t)) => {
                        self.state.borrow_mut().toggle(t);
                        refresh(hwnd);
                    }
                    Some(Id::Picker(k)) => {
                        let (r, (names, current)) = {
                            let s = self.state.borrow();
                            (s.rect_of(Id::Picker(k)).unwrap_or_default(), s.choices(k))
                        };
                        if let Some(i) = popup(hwnd, r, &names, current) {
                            self.state.borrow_mut().pick(k, i);
                            refresh(hwnd);
                        }
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wparam.0 == PILL_TIMER => {
                let moving = {
                    let mut s = self.state.try_borrow_mut().ok()?;
                    s.preview.liquid.step(Instant::now());
                    s.moving()
                };
                if !moving {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), PILL_TIMER);
                    }
                }
                refresh(hwnd);
                Some(LRESULT(0))
            }
            WM_KEYDOWN => {
                let vk = VIRTUAL_KEY(wparam.0 as u16);
                if vk == VK_RETURN || vk == VK_ESCAPE {
                    unsafe {
                        let _ = DestroyWindow(hwnd);
                    }
                } else if vk == VK_TAB {
                    // Ctrl+Tab / Ctrl+Shift+Tab move between tabs.
                    let back = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                    let mut s = self.state.borrow_mut();
                    let next = if back { (s.tab + TABS.len() - 1) % TABS.len() } else { (s.tab + 1) % TABS.len() };
                    s.select(Group::Tabs, next);
                    animate(hwnd, &s);
                    refresh(hwnd);
                }
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                let r = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                refresh(hwnd);
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                apply_theme(hwnd);
                refresh(hwnd);
                None
            }
            WM_DESTROY => {
                events::post(UiEvent::WindowClosed { window: hwnd.0 as isize });
                None
            }
            _ => None,
        }
    }
}

/// Opens the Settings window (or brings the open one forward).
pub fn open(settings: &Settings) {
    if let Some(hwnd) = unsafe { FindWindowW(None, &HSTRING::from(TITLE)) }.ok().filter(|h| !h.is_invalid()) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(hwnd);
        }
        return;
    }
    let items = [Format::Png, Format::Webp, Format::Heic, Format::Tiff, Format::Avif, Format::Bmp, Format::Pdf, Format::Docx]
        .into_iter()
        .map(WheelItem::Convert)
        .collect();
    let mut preview = WheelModel::new(WheelMode::Conversions, items);
    preview.selected = Some(1);
    preview.hub = vec!["2.4 MB".into()];
    preview.estimates.insert(WheelItem::Convert(Format::Webp), Some("1.1 MB".into()));
    preview.on_glass_window = true;
    preview.settle_liquid();
    let state = Rc::new(RefCell::new(State {
        settings: settings.clone(),
        login: system::launch_at_login(),
        tab: GENERAL,
        pill: None,
        pressed: None,
        dragging: false,
        preview,
        scale: 1.0,
    }));

    let (cx, cy) = system::cursor_pos();
    let scale = system::scale_at(cx, cy);
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let ex = WINDOW_EX_STYLE(0);
    let mut r = RECT { left: 0, top: 0, right: (W * scale).round() as i32, bottom: (H * scale).round() as i32 };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, (scale * 96.0) as u32);
    }
    let (ww, wh) = (r.right - r.left, r.bottom - r.top);
    let work = system::work_area_at(cx, cy);
    let x = (work.left + work.right - ww) / 2;
    let y = ((work.top + work.bottom - wh) / 2).max(work.top);
    let hwnd = window::create(ex, style, TITLE, x, y, ww, wh, None, Rc::new(Messages { state }));
    apply_theme(hwnd);
    let s = scale_of(hwnd);
    if (s - scale).abs() > 0.01 {
        let mut r = RECT { left: 0, top: 0, right: (W * s).round() as i32, bottom: (H * s).round() as i32 };
        unsafe {
            let _ = AdjustWindowRectExForDpi(&mut r, style, false, ex, GetDpiForWindow(hwnd));
            let _ = SetWindowPos(hwnd, None, 0, 0, r.right - r.left, r.bottom - r.top, SWP_NOMOVE | SWP_NOZORDER);
        }
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
    }
}
