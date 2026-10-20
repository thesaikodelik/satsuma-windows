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
const TABS: [&str; 5] = ["General", "Appearance", "Shortcuts", "Files", "About"];
const GENERAL: usize = 0;
const APPEARANCE: usize = 1;
const SHORTCUTS: usize = 2;
const FILES: usize = 3;
const ABOUT: usize = 4;

/// Settings › About: who made Satsuma (role, name, link).
const CREDITS: [(&str, &str, &str); 2] = [
    ("Created by (macOS)", "sonatina-dream", "https://github.com/sonatina-dream/satsuma"),
    ("Windows edition by", "thesaikodelik", "https://github.com/thesaikodelik/satsuma-windows"),
];
const SPONSOR_URL: &str = "https://github.com/sponsors/thesaikodelik";
/// The credit rows start here; each is `CREDIT_H` tall.
const CREDITS_Y: f32 = 222.0;
const CREDIT_H: f32 = 48.0;
const UPDATES_Y: f32 = 336.0;

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
    /// About: a credit row (0, 1) or the sponsor link (2).
    Link(usize),
    /// About: Check Now, or Restart to Update once one is downloaded.
    UpdateButton,
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
    /// When the how-to animation started.
    tutorial: Instant,
    /// The control under the pointer (About's links and buttons light up).
    hover: Option<Id>,
    /// The app icon for About (RGBA, `LOGO_PX` square).
    logo: Vec<u8>,
}

const LOGO_PX: u32 = 160;

/// Rows hold a label on the left and a control on the right.
const ROW_LABEL_X: f32 = 20.0;
const CONTROL_W: f32 = 220.0;
const CONTROL_X: f32 = W - 20.0 - CONTROL_W;
const PREVIEW_Y: f32 = 54.0;
/// The sound switch's row on the Shortcuts tab.
const SOUND_Y: f32 = 424.0;

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
            ABOUT => {
                for k in 0..CREDITS.len() {
                    v.push((Id::Link(k), (16.0, CREDITS_Y + k as f32 * CREDIT_H, W - 32.0, CREDIT_H - 4.0)));
                }
                v.push((Id::UpdateButton, (W - 20.0 - 150.0, UPDATES_Y + 28.0, 150.0, 30.0)));
                v.push((Id::Link(2), (14.0, H - 44.0, 150.0, 26.0)));
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

    /// Only the how-to animation is changing, so frames can redraw just its area.
    fn only_tutorial(&self) -> bool {
        self.tab == SHORTCUTS && !self.pill.is_some_and(|(_, _, t)| t.elapsed().as_secs_f32() < PILL_SECONDS) && !self.preview.liquid.moving() && self.pressed.is_none()
    }

    fn moving(&self) -> bool {
        (self.tab == ABOUT && matches!(super::update::status(), super::update::Status::Checking | super::update::Status::Downloading(_)))
            || self.tab == SHORTCUTS || self.pill.is_some_and(|(_, _, t)| t.elapsed().as_secs_f32() < PILL_SECONDS) || self.preview.liquid.moving()
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
                if i == SHORTCUTS && self.tab != SHORTCUTS {
                    self.tutorial = Instant::now();
                }
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
            ABOUT => self.paint_about(p, pal),
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

    /// The animated how-to: a pointer drags a document, the wheel key is tapped once while
    /// the click is held, and the file is dropped on a petal. Loops every `LOOP` seconds.
    fn paint_tutorial(&self, p: &Painter, pal: &Palette, top: f32) {
        const LOOP: f32 = 6.0;
        let t = self.tutorial.elapsed().as_secs_f32() % LOOP;
        let ink = pal.secondary;
        let (x0, y0, w, h) = (20.0, top, W - 40.0, 140.0);
        // The stage.
        p.fill_round_rect(x0, y0, w, h, 16.0, if pal.glass { pal.control } else { pal.separator.alpha(0.35) });
        if pal.glass {
            p.stroke_round_rect(x0 + 0.5, y0 + 0.5, w - 1.0, h - 1.0, 15.5, pal.rim, 1.0);
        }
        // Fade the whole scene in and out around the loop.
        let fade = (t / 0.3).min(1.0).min(((LOOP - t) / 0.4).min(1.0)).max(0.0);
        let ink = ink.alpha(fade);
        let accent = ACCENT.alpha(fade);

        let doc_home = (x0 + 46.0, y0 + 30.0);
        let doc_size = (30.0, 38.0);
        let doc_mid = (doc_home.0 + doc_size.0 / 2.0, doc_home.1 + doc_size.1 / 2.0);
        let wheel_c = (x0 + w * 0.62, y0 + 62.0);
        let petal = 1usize;
        let geo_r = |s: f32| WheelGeometry { inner: (14.0 * s) as f64, outer: (44.0 * s) as f64, gap: 2.5, outer_corner: 6.0 * s as f64, inner_corner: 2.5, backdrop: (47.0 * s) as f64 };
        let petal_mid = {
            let lp = geo_r(1.0).label_position(petal, 6);
            (wheel_c.0 + lp.x as f32, wheel_c.1 - lp.y as f32)
        };
        let start = (x0 + 150.0, y0 + 118.0);

        // The pointer's path and the dragged document.
        let lerp = |a: (f32, f32), b: (f32, f32), k: f32| (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k);
        let span = |a: f32, b: f32| ease(((t - a) / (b - a)).clamp(0.0, 1.0));
        let pointer = if t < 0.7 {
            lerp(start, doc_mid, span(0.0, 0.7))
        } else if t < 2.0 {
            lerp(doc_mid, wheel_c, span(0.85, 2.0))
        } else if t < 3.6 {
            lerp(wheel_c, petal_mid, span(2.75, 3.5))
        } else {
            lerp(petal_mid, (x0 + w - 40.0, y0 + 112.0), span(4.3, 5.2))
        };
        let pressed = (0.75..3.7).contains(&t);
        let dropped = t >= 3.7;

        // The original stays put; a copy travels with the pointer.
        draw_document(p, doc_home.0, doc_home.1, doc_size.0, doc_size.1, ink.alpha(if pressed { 0.45 } else { 1.0 }));
        // After the drop the result appears beside the original.
        let made = span(4.0, 4.4);
        if made > 0.0 {
            let k = spring_pop(made);
            let (dw, dh) = (doc_size.0 * k, doc_size.1 * k);
            let (nx, ny) = (doc_home.0 + 44.0 + (doc_size.0 - dw) / 2.0, doc_home.1 + (doc_size.1 - dh) / 2.0);
            draw_document(p, nx, ny, dw, dh, accent);
            if k > 0.7 {
                p.text("PNG", (nx - 6.0, ny + dh + 2.0, dw + 12.0, 14.0), 8.5, Weight::Bold, accent, Align::Center, false);
            }
        }

        // The keys: the Convert key goes down once while the click is held.
        let keys = [self.settings.wheels.conversions.label(), self.settings.wheels.tools.label(), self.settings.wheels.actions.label()];
        let key_down = (2.15..2.45).contains(&t);
        let keys_on = span(1.7, 2.0) * (1.0 - span(3.7, 4.0));
        if keys_on > 0.0 {
            let mut kx = x0 + 18.0;
            for (i, k) in keys.iter().enumerate() {
                let kw = 18.0 + k.len() as f32 * 6.5;
                let down = i == 0 && key_down;
                let (ky, a) = (y0 + h - 34.0 + if down { 2.0 } else { 0.0 }, keys_on * fade);
                if i == 0 && (2.1..2.6).contains(&t) {
                    p.fill_round_rect(kx, ky, kw, 22.0, 6.0, ACCENT.alpha(0.22 * a));
                }
                p.stroke_round_rect(kx, y0 + h - 31.0, kw, 22.0, 6.0, ink.alpha(0.35 * a), 1.0);
                p.stroke_round_rect(kx, ky, kw, 22.0, 6.0, if i == 0 { ACCENT.alpha(a) } else { ink.alpha(a) }, 1.4);
                p.text(k, (kx, ky, kw, 22.0), 10.0, Weight::Semibold, if i == 0 { ACCENT.alpha(a) } else { ink.alpha(a) }, Align::Center, false);
                if i == 0 && (2.05..2.6).contains(&t) {
                    // Tap marks.
                    for dx in [-8.0f32, 0.0, 8.0] {
                        let cx = kx + kw / 2.0 + dx;
                        p.line(cx + dx * 0.15, ky - 4.0, cx + dx * 0.35, ky - 10.0, ACCENT.alpha(a), 1.5);
                    }
                }
                kx += kw + 8.0;
            }
        }

        // The wheel pops open after the tap and closes after the drop.
        let open = spring_pop(span(2.3, 2.75)) * (1.0 - span(3.9, 4.3));
        if open > 0.01 {
            let g = geo_r(open.max(0.05));
            p.fill_circle(wheel_c.0, wheel_c.1, g.backdrop as f32, ink.alpha(0.08));
            p.stroke_circle(wheel_c.0, wheel_c.1, g.backdrop as f32, ink, 1.2);
            let lit = t >= 3.3 && t < 4.3;
            for i in 0..6 {
                if let Some(geo) = p.path(&g.petal_path(i, 6), wheel_c.0, wheel_c.1) {
                    if i == petal && lit {
                        p.fill_geometry(&geo, accent);
                    }
                    p.stroke_geometry(&geo, ink, 1.0);
                }
            }
        }

        // The dragged copy, shrinking into the petal on drop.
        if pressed || (dropped && t < 4.0) {
            let k = if dropped { 1.0 - span(3.7, 4.0) } else { 0.85 };
            if k > 0.02 {
                let (dw, dh) = (doc_size.0 * k, doc_size.1 * k);
                let at = if dropped { petal_mid } else { pointer };
                draw_document(p, at.0 - dw * 0.7, at.1 - dh * 0.75, dw, dh, ink);
            }
        }
        // The pointer, with a ripple while the button is held.
        if pressed {
            let r = 6.0 + 3.0 * ((t * 6.0).sin() * 0.5 + 0.5);
            p.stroke_circle(pointer.0, pointer.1, r, accent.alpha(0.6), 1.2);
        }
        if fade > 0.0 {
            draw_cursor(p, pointer.0, pointer.1, ink, pal);
        }

        // Step dots and the caption for the current step, cross-fading.
        let step = if t < 1.95 { 0 } else if t < 3.55 { 1 } else { 2 };
        let edges = [0.0, 1.95, 3.55, LOOP];
        let local = ((t - edges[step]) / 0.25).min((edges[step + 1] - t) / 0.25).clamp(0.0, 1.0);
        let captions = [
            "Drag a document.".to_string(),
            format!("Keep holding the click and tap {}, {} or {} once.", keys[0], keys[1], keys[2]),
            "Drop the file onto a petal.".to_string(),
        ];
        let cy = y0 + h + 8.0;
        for k in 0..3 {
            let on = k == step;
            p.fill_circle(x0 + 6.0 + k as f32 * 12.0, cy + 10.0, if on { 4.0 } else { 3.0 }, if on { ACCENT } else { pal.tertiary });
        }
        p.text(&captions[step], (x0 + 44.0, cy, w - 44.0, 20.0), 12.0, Weight::Semibold, pal.label.alpha(local), Align::Leading, false);
    }

    /// About, after Tachyon's: the icon, name and version; who made it; updates; a coffee.
    fn paint_about(&self, p: &Painter, pal: &Palette) {
        use super::update::{status, Status, CURRENT};
        let c = W / 2.0;
        // The icon on a soft citrus glow.
        let (icon, iy) = (72.0, 58.0);
        for k in (1..=6).rev() {
            p.fill_circle(c, iy + icon / 2.0, icon / 2.0 + k as f32 * 5.0, ACCENT.alpha(0.035));
        }
        p.image(&self.logo, LOGO_PX, LOGO_PX, (c - icon / 2.0, iy, icon, icon));
        p.text("Satsuma", (0.0, 136.0, W, 28.0), 20.0, Weight::Semibold, pal.label, Align::Center, false);
        p.text(&format!("Version {CURRENT}"), (0.0, 164.0, W, 18.0), 12.0, Weight::Regular, pal.secondary, Align::Center, false);
        p.text("Drag files, tap a key, drop on a petal.", (0.0, 184.0, W, 18.0), 12.0, Weight::Regular, pal.tertiary, Align::Center, false);
        p.line(16.0, 212.5, W - 16.0, 212.5, pal.separator, 1.0);

        // Who made it: a role over a name, opening their page.
        for (k, (role, name, _)) in CREDITS.iter().enumerate() {
            let Some(r) = self.rect_of(Id::Link(k)) else { continue };
            if self.hover == Some(Id::Link(k)) || self.pressed == Some(Id::Link(k)) {
                p.fill_round_rect(r.0, r.1, r.2, r.3, if pal.glass { 14.0 } else { 6.0 }, pal.control.alpha(if pal.glass { 1.8 } else { 1.0 }));
            }
            p.text(role, (r.0 + 12.0, r.1 + 5.0, r.2 - 50.0, 16.0), 11.0, Weight::Regular, pal.secondary, Align::Leading, false);
            p.text(name, (r.0 + 12.0, r.1 + 20.0, r.2 - 50.0, 20.0), 13.0, Weight::Semibold, pal.label, Align::Leading, false);
            p.icon('\u{E8A7}', r.0 + r.2 - 20.0, r.1 + r.3 / 2.0, 13.0, pal.secondary);
        }

        // Updates.
        p.text("UPDATES", (ROW_LABEL_X, UPDATES_Y, W - 40.0, 22.0), 11.0, Weight::Semibold, pal.secondary, Align::Leading, false);
        let st = status();
        let line = match &st {
            Status::Idle => "Satsuma looks for new versions by itself.".to_string(),
            Status::Checking => "Checking for updates…".to_string(),
            Status::UpToDate => format!("Satsuma {CURRENT} is up to date."),
            Status::Downloading(v) => format!("Downloading Satsuma {v}…"),
            Status::Ready(v) => format!("Satsuma {v} is ready to install."),
            Status::Failed(e) => format!("Couldn't check for updates ({e})"),
        };
        let busy = matches!(st, Status::Checking | Status::Downloading(_));
        let mut tx = ROW_LABEL_X;
        if busy {
            // Three dots hop while it works, as in the wheel's hub.
            let phase = self.tutorial.elapsed().as_secs_f32();
            for k in 0..3 {
                let t = (phase * 2.4 - k as f32 * 0.18).rem_euclid(1.0);
                let hop = if t < 0.5 { (t * 2.0 * std::f32::consts::PI).sin().max(0.0) } else { 0.0 };
                p.fill_circle(ROW_LABEL_X + 3.0 + k as f32 * 8.0, UPDATES_Y + 43.0 - hop * 4.0, 2.4, ACCENT.alpha(0.55 + 0.45 * hop));
            }
            tx += 28.0;
        }
        let color = if matches!(st, Status::Failed(_)) { pal.error } else { pal.label };
        p.text(&line, (tx, UPDATES_Y + 28.0, W - 20.0 - 150.0 - 12.0 - tx, 30.0), 12.0, Weight::Regular, color, Align::Leading, true);
        if let Some(r) = self.rect_of(Id::UpdateButton) {
            let ready = matches!(st, Status::Ready(_));
            let (radius, down) = (if pal.glass { r.3 / 2.0 } else { 6.0 }, self.pressed == Some(Id::UpdateButton));
            if ready {
                p.fill_round_rect(r.0, r.1, r.2, r.3, radius, if down { Rgba(0.85, 0.36, 0.07, 1.0) } else { ACCENT });
                p.text("Restart to Update", r, 12.5, Weight::Semibold, Rgba::WHITE, Align::Center, false);
            } else {
                let lit = self.hover == Some(Id::UpdateButton) && !busy;
                p.fill_round_rect(r.0, r.1, r.2, r.3, radius, pal.control.alpha(if lit || down { 2.2 } else if pal.glass { 1.4 } else { 1.0 }));
                p.stroke_round_rect(r.0 + 0.5, r.1 + 0.5, r.2 - 1.0, r.3 - 1.0, radius - 0.5, if pal.glass { pal.rim } else { pal.separator }, 1.0);
                p.text("Check Now", r, 12.5, Weight::Semibold, if busy { pal.tertiary } else { pal.label }, Align::Center, false);
            }
        }

        // A coffee for the Windows edition.
        if let Some(r) = self.rect_of(Id::Link(2)) {
            let lit = self.hover == Some(Id::Link(2));
            p.text("\u{2615}", (r.0 + 4.0, r.1, 22.0, r.3), 15.0, Weight::Regular, Rgba::hex(0xB7793E, 1.0), Align::Leading, false);
            p.text("Buy me a coffee", (r.0 + 26.0, r.1, r.2 - 26.0, r.3), 12.0, Weight::Regular, if lit { pal.label } else { pal.secondary }, Align::Leading, false);
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
        if self.tab == ABOUT {
            let hover = self.hit(x, y).filter(|id| matches!(id, Id::Link(_) | Id::UpdateButton));
            if hover != self.hover {
                self.hover = hover;
                return true;
            }
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

/// Smooth in and out.
fn ease(k: f32) -> f32 {
    k * k * (3.0 - 2.0 * k)
}

/// 0 → 1 with a little springy overshoot (for pop-ins).
fn spring_pop(k: f32) -> f32 {
    if k <= 0.0 {
        return 0.0;
    }
    let k = k.min(1.0);
    1.0 - (-6.0 * k).exp() * (11.0 * k).cos() * (1.0 - k)
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
    /// The last frame, kept so animation frames redraw only what moves.
    surface: RefCell<Option<Surface>>,
}

/// The how-to scene and its caption (DIPs).
const TUTORIAL_CLIP: (f32, f32, f32, f32) = (14.0, 234.0, W - 28.0, 180.0);

fn tutorial_rect_px(scale: f32) -> RECT {
    let (x, y, w, h) = TUTORIAL_CLIP;
    RECT { left: (x * scale).floor() as i32, top: (y * scale).floor() as i32, right: ((x + w) * scale).ceil() as i32, bottom: ((y + h) * scale).ceil() as i32 }
}

/// General, or (debug builds) the tab number in SATSUMA_OPEN_SETTINGS.
fn initial_tab() -> usize {
    #[cfg(debug_assertions)]
    if let Some(t) = std::env::var("SATSUMA_OPEN_SETTINGS").ok().and_then(|v| v.parse::<usize>().ok()).filter(|t| *t < TABS.len()) {
        return t;
    }
    GENERAL
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
        unsafe { SetTimer(Some(hwnd), PILL_TIMER, frame_ms(s), None) };
    }
}

/// The pill and the liquid run at the full frame rate; the line-drawn how-to needs only 30.
fn frame_ms(s: &State) -> u32 {
    if s.tab == ABOUT && s.pill.is_none_or(|(_, _, t)| t.elapsed().as_secs_f32() >= PILL_SECONDS) {
        // Only About's waiting dots move.
        33
    } else if s.only_tutorial() {
        33
    } else {
        10
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
                    if let (Ok(s), Ok(mut cached)) = (self.state.try_borrow(), self.surface.try_borrow_mut()) {
                        let pal = Palette::current();
                        let scale = scale_of(hwnd);
                        let (w, h) = (r.right.max(1), r.bottom.max(1));
                        // Reuse the last frame while the size holds; then a repaint of just
                        // the how-to area (what the animation invalidates) redraws only it.
                        let keep = cached.as_ref().is_some_and(|c| c.width == w && c.height == h && !c.is_lost());
                        if !keep {
                            *cached = Surface::new(w, h).ok();
                        }
                        if let Some(surface) = cached.as_ref() {
                            let t = tutorial_rect_px(scale);
                            let dirty = ps.rcPaint;
                            let inside = dirty.left >= t.left && dirty.top >= t.top && dirty.right <= t.right && dirty.bottom <= t.bottom;
                            if keep && inside && s.tab == SHORTCUTS {
                                surface.draw_clipped(scale, TUTORIAL_CLIP, |p| {
                                    p.fill_round_rect(0.0, 0.0, W, H, 0.0, if pal.glass { pal.backdrop } else { Rgba(pal.panel.0, pal.panel.1, pal.panel.2, 1.0) });
                                    s.paint_tutorial(p, &pal, 240.0);
                                });
                                surface.blit_rect(dc, t);
                            } else {
                                surface.draw(scale, |p| s.paint(p, &pal));
                                surface.blit(dc, 0, 0);
                            }
                        }
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
                    Some(Id::Link(k)) => {
                        let url = CREDITS.get(k).map(|c| c.2).unwrap_or(SPONSOR_URL);
                        system::open_item(std::path::Path::new(url));
                    }
                    Some(Id::UpdateButton) => {
                        use super::update::Status;
                        match super::update::status() {
                            Status::Ready(_) => events::post(UiEvent::RestartToUpdate),
                            Status::Checking | Status::Downloading(_) => {}
                            _ => {
                                super::update::check_now();
                                // Show "Checking…" at once; the timer follows the check.
                                std::thread::sleep(std::time::Duration::from_millis(30));
                                let mut s = self.state.borrow_mut();
                                s.tutorial = Instant::now();
                                animate(hwnd, &s);
                            }
                        }
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
                let (moving, only_tutorial, ms) = {
                    let mut s = self.state.try_borrow_mut().ok()?;
                    s.preview.liquid.step(Instant::now());
                    (s.moving(), s.only_tutorial(), frame_ms(&s))
                };
                unsafe {
                    if !moving {
                        let _ = KillTimer(Some(hwnd), PILL_TIMER);
                    } else {
                        SetTimer(Some(hwnd), PILL_TIMER, ms, None);
                    }
                }
                if unsafe { IsIconic(hwnd) }.as_bool() {
                    // Minimised: nothing to show.
                } else if only_tutorial {
                    let r = tutorial_rect_px(scale_of(hwnd));
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), Some(&r), false);
                    }
                } else {
                    refresh(hwnd);
                }
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
        tab: initial_tab(),
        pill: None,
        pressed: None,
        dragging: false,
        preview,
        scale: 1.0,
        tutorial: Instant::now(),
        hover: None,
        logo: super::icon::app_icon(LOGO_PX),
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
    let hwnd = window::create(ex, style, TITLE, x, y, ww, wh, None, Rc::new(Messages { state: state.clone(), surface: RefCell::new(None) }));
    apply_theme(hwnd);
    animate(hwnd, &state.borrow());
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
