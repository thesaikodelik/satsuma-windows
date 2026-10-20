//! Light and dark palettes standing in for the AppKit system colours.

use super::gfx::Rgba;
use satsuma_core::settings::Appearance;
use std::sync::atomic::{AtomicU8, Ordering};

static APPEARANCE: AtomicU8 = AtomicU8::new(0);

/// Applies the appearance setting to every surface drawn from now on.
pub fn set_appearance(a: Appearance) {
    APPEARANCE.store(Appearance::ALL.iter().position(|x| *x == a).unwrap_or(0) as u8, Ordering::Relaxed);
}

pub fn appearance() -> Appearance {
    let a = Appearance::ALL[(APPEARANCE.load(Ordering::Relaxed) as usize).min(2)];
    // Blur behind windows needs Windows 10 1809 or later.
    if a != Appearance::Solid && !super::system::supports_blur() {
        return Appearance::Solid;
    }
    a
}

/// Tangerine orange, as in the macOS app.
pub const ACCENT: Rgba = Rgba(1.0, 0.43, 0.09, 1.0);

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Surfaces sit on a blurred backdrop (Liquid Glass) and get a bright rim.
    pub glass: bool,
    /// The 1 px light rim around glass layers (Tachyon's "highlight").
    pub rim: Rgba,
    /// The frosted disc behind the petals (`windowBackgroundColor`).
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
    pub fn new(dark: bool) -> Palette {
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

    /// Light glass: white tints over the blur, black ink (Tachyon's "Liquid Glass").
    pub fn liquid_glass() -> Palette {
        let ink = |a: f32| Rgba(0.0, 0.0, 0.0, a);
        Palette {
            dark: false,
            glass: true,
            rim: Rgba(1.0, 1.0, 1.0, 0.80),
            backdrop: Rgba(1.0, 1.0, 1.0, 0.25),
            separator: ink(0.08),
            control: Rgba(1.0, 1.0, 1.0, 0.35),
            label: ink(0.90),
            secondary: ink(0.60),
            tertiary: ink(0.40),
            error: Rgba::hex(0xc42b1c, 1.0),
            panel: Rgba(1.0, 1.0, 1.0, 0.25),
        }
    }

    /// Smoky glass: a dark tint over the blur, white ink ("Liquid Glass Smoke").
    pub fn liquid_smoke() -> Palette {
        let ink = |a: f32| Rgba(1.0, 1.0, 1.0, a);
        Palette {
            dark: true,
            glass: true,
            rim: ink(0.25),
            backdrop: Rgba::hex(0x101014, 0.75),
            separator: ink(0.12),
            control: ink(0.06),
            label: ink(1.0),
            secondary: ink(0.72),
            tertiary: ink(0.45),
            error: Rgba::hex(0xff5c6c, 1.0),
            panel: Rgba::hex(0x101014, 0.75),
        }
    }

    pub fn current() -> Palette {
        match appearance() {
            Appearance::LiquidGlass => Palette::liquid_glass(),
            Appearance::LiquidSmoke => Palette::liquid_smoke(),
            Appearance::Solid => Palette::new(super::system::is_dark()),
        }
    }

    /// The palette an appearance would use (for offscreen renders).
    pub fn for_appearance(a: Appearance, dark: bool) -> Palette {
        match a {
            Appearance::LiquidGlass => Palette::liquid_glass(),
            Appearance::LiquidSmoke => Palette::liquid_smoke(),
            Appearance::Solid => Palette::new(dark),
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
