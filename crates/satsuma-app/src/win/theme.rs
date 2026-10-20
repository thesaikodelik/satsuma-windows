//! The two looks: Liquid (30nama's drawn glass over a blur, with adjustable transparency)
//! and Solid (opaque, following the system light/dark mode).

use super::gfx::Rgba;
use satsuma_core::settings::Appearance;
use std::sync::atomic::{AtomicU8, Ordering};

static APPEARANCE: AtomicU8 = AtomicU8::new(0);
static TRANSPARENCY: AtomicU8 = AtomicU8::new(45);

/// Applies the appearance setting to every surface drawn from now on.
pub fn set_appearance(a: Appearance) {
    APPEARANCE.store(Appearance::ALL.iter().position(|x| *x == a).unwrap_or(0) as u8, Ordering::Relaxed);
}

/// How see-through Liquid is, 0–100.
pub fn set_transparency(t: u8) {
    TRANSPARENCY.store(t.min(100), Ordering::Relaxed);
}

pub fn transparency() -> f32 {
    TRANSPARENCY.load(Ordering::Relaxed) as f32 / 100.0
}

pub fn appearance() -> Appearance {
    let a = Appearance::ALL[(APPEARANCE.load(Ordering::Relaxed) as usize).min(Appearance::ALL.len() - 1)];
    // Blur behind windows needs Windows 10 1809 or later.
    if a == Appearance::Liquid && !super::system::supports_blur() {
        return Appearance::Solid;
    }
    a
}

/// How strongly Liquid blurs what is behind it (DIPs): clearer glass blurs less.
pub fn blur_sigma() -> f32 {
    3.0 + 12.0 * (1.0 - transparency())
}

/// Tangerine orange, as in the macOS app.
pub const ACCENT: Rgba = Rgba(1.0, 0.43, 0.09, 1.0);

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Liquid: surfaces sit on a blurred backdrop, with 30nama's glare and lit rims.
    pub glass: bool,
    /// The light rim around glass surfaces.
    pub rim: Rgba,
    /// The disc behind the petals, or the tint over the blur.
    pub backdrop: Rgba,
    pub separator: Rgba,
    /// Petals and the hub (`controlBackgroundColor`).
    pub control: Rgba,
    pub label: Rgba,
    pub secondary: Rgba,
    pub tertiary: Rgba,
    pub error: Rgba,
    /// Panel background (HUD).
    pub panel: Rgba,
}

impl Palette {
    /// Solid, light or dark. Transparency fades the wheel and panel surfaces (no blur):
    /// opaque at 0, today's look at 45, faint at 100.
    pub fn solid(dark: bool, transparency: f32) -> Palette {
        let t = transparency.clamp(0.0, 1.0);
        let o = if t <= 0.45 { 1.0 - t * (0.2 / 0.45) } else { 0.8 - (t - 0.45) * (0.65 / 0.55) };
        let mut p = Palette::solid_base(dark);
        p.backdrop.3 = o;
        p.control.3 = (o + 0.2).min(1.0);
        p.panel.3 = (o + 0.17).min(0.98);
        p
    }

    fn solid_base(dark: bool) -> Palette {
        if dark {
            Palette {
                dark,
                glass: false,
                rim: Rgba(1.0, 1.0, 1.0, 0.0),
                backdrop: Rgba::hex(0x202020, 0.80),
                separator: Rgba(1.0, 1.0, 1.0, 0.14),
                control: Rgba::hex(0x3a3a3a, 1.0),
                label: Rgba::hex(0xf3f3f3, 1.0),
                secondary: Rgba(1.0, 1.0, 1.0, 0.64),
                tertiary: Rgba(1.0, 1.0, 1.0, 0.40),
                error: Rgba::hex(0xff6b5f, 1.0),
                panel: Rgba::hex(0x2b2b2b, 0.97),
            }
        } else {
            Palette {
                dark,
                glass: false,
                rim: Rgba(1.0, 1.0, 1.0, 0.0),
                backdrop: Rgba::hex(0xf3f3f3, 0.78),
                separator: Rgba(0.0, 0.0, 0.0, 0.13),
                control: Rgba::hex(0xffffff, 1.0),
                label: Rgba::hex(0x1b1b1b, 1.0),
                secondary: Rgba(0.0, 0.0, 0.0, 0.58),
                tertiary: Rgba(0.0, 0.0, 0.0, 0.32),
                error: Rgba::hex(0xc42b1c, 1.0),
                panel: Rgba::hex(0xf9f9f9, 0.98),
            }
        }
    }

    /// Liquid (30nama's glass, dark): the `#1A1C22` tint is 55 % at the default
    /// transparency (45), from 95 % (0, frosted) down to 5 % (100, clear).
    pub fn liquid(transparency: f32) -> Palette {
        let tint = 0.95 - 0.9 * transparency.clamp(0.0, 1.0);
        let text = |a: f32| Rgba::hex(0xF4F1EA, a);
        Palette {
            dark: true,
            glass: true,
            rim: Rgba(1.0, 1.0, 1.0, 0.14),
            backdrop: Rgba::hex(0x1A1C22, tint),
            separator: Rgba(1.0, 1.0, 1.0, 0.10),
            control: Rgba(1.0, 1.0, 1.0, 0.04),
            label: text(1.0),
            secondary: text(0.70),
            tertiary: text(0.45),
            error: Rgba::hex(0xF0454A, 1.0),
            panel: Rgba::hex(0x1A1C22, tint),
        }
    }

    pub fn current() -> Palette {
        Palette::for_appearance(appearance(), super::system::is_dark())
    }

    pub fn for_appearance(a: Appearance, dark: bool) -> Palette {
        match a {
            Appearance::Liquid => Palette::liquid(transparency()),
            Appearance::Solid => Palette::solid(dark, transparency()),
        }
    }
}

/// Segoe Fluent Icons / MDL2 Assets glyphs for the SF Symbols the core names.
pub fn glyph(symbol: &str) -> char {
    let code = match symbol {
        "arrow.down.right.and.arrow.up.left" => 0xE73F, // BackToWindow
        "tag" => 0xE8EC,
        "slider.horizontal.3" => 0xE9E9, // Equalizer
        "pencil.and.outline" => 0xE70F, // Edit
        "photo.artframe" => 0xEB9F,     // Photo2
        "crop" => 0xE7A8,
        "eye.slash" => 0xED1A, // Hide
        "waveform" => 0xE8D6,  // Audio
        "scissors" => 0xE8C6,  // Cut
        "hifispeaker.2" => 0xE7F5,
        "waveform.badge.minus" => 0xE7ED,
        "speaker.slash.fill" => 0xE74F, // Mute
        "speedometer" => 0xEC4A,        // SpeedHigh
        "camera" => 0xE722,
        "film" | "film.stack" => 0xE8B2, // Movies
        "rectangle.split.2x1" => 0xE89F, // ClosePane
        "square.grid.2x2" => 0xF0E2,     // GridView
        "doc.on.doc" => 0xE8C8,          // Copy
        "doc.badge.plus" => 0xEA90,      // PDF
        "rectangle.3.group" => 0xE80A,   // ViewAll
        "archivebox" => 0xF012,          // ZipFolder
        "airplayaudio" => 0xE72D,        // Share
        "pin.fill" => 0xE718,            // Pin
        "folder" => 0xE8DE,              // MoveToFolder
        "doc.text" => 0xE71B,            // Link
        "terminal" => 0xE756,            // CommandPrompt
        _ => 0xE10C,                     // More
    };
    char::from_u32(code).unwrap_or('•')
}
