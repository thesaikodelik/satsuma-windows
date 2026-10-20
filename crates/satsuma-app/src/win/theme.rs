//! Light and dark palettes standing in for the AppKit system colours.

use super::gfx::Rgba;

/// Tangerine orange, as in the macOS app.
pub const ACCENT: Rgba = Rgba(1.0, 0.43, 0.09, 1.0);

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
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

    pub fn current() -> Palette {
        Palette::new(super::system::is_dark())
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
