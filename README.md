<p align="center">
  <img src="docs/media/logo-banner.jpg" alt="Satsuma: drag, tap, drop" width="100%">
</p>

<p align="center">
  <a href="https://github.com/thesaikodelik/satsuma-windows/releases/latest/download/Satsuma-Setup.exe"><img src="https://img.shields.io/badge/download-Satsuma--Setup.exe-ff6e17?style=for-the-badge&logo=windows11&logoColor=white" alt="Download Satsuma for Windows"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-11-0078d4?logo=windows11&logoColor=white" alt="Windows 11">
  <img src="https://img.shields.io/badge/version-1.0.0-ff6e17" alt="Version 1.0.0">
  <img src="https://img.shields.io/badge/license-MIT-2ea44f" alt="MIT license">
  <img src="https://img.shields.io/badge/made%20with-Rust-b7410e?logo=rust&logoColor=white" alt="Made with Rust">
  <img src="https://img.shields.io/badge/size-12%20MB-6e40c9" alt="12 MB">
</p>

<p align="center">
  <b>English</b> · <a href="README.fa.md">فارسی</a>
</p>

<p align="center">
  <img src="docs/media/demo.gif" width="100%" alt="Dragging Sunset.jpg out of File Explorer and tapping Shift opens the Convert wheel under the pointer. The orange highlight flows from petal to petal and the hub shows each result's size; Ctrl and Alt switch to the Tools and Actions wheels. Dropping on PNG converts the photo and Sunset.png appears next to it.">
  <br>
  <sub><a href="docs/media/demo.mp4">Watch it as a video (MP4)</a></sub>
</p>

**Satsuma** turns every file drag into a shortcut. While you drag files, tap **Shift**, **Ctrl** or
**Alt** and a wheel of options opens right under your pointer. Drop onto a petal and the result is
saved next to the original: a photo becomes a PNG, a video gets smaller, PDFs merge, a picture gets a
background. Everything happens on your PC; nothing is uploaded.

This is the Windows edition of [Satsuma](https://github.com/sonatina-dream/satsuma) by
sonatina-dream, rebuilt natively for Windows 11 with a *Liquid Glass* look of its own.

<p align="center"><img src="docs/media/highlights.png" width="100%" alt="Every format, one tap, private, updates itself"></p>

---

## 🎡 Three wheels, one gesture

<table>
  <tr>
    <td width="33%" align="center"><img src="docs/media/wheel-convert.png" alt="The Convert wheel for a JPG, with WEBP lit and its size, 15 KB, in the hub"></td>
    <td width="33%" align="center"><img src="docs/media/wheel-tools.png" alt="The Tools wheel for a JPG, with Crop lit"></td>
    <td width="33%" align="center"><img src="docs/media/wheel-actions.png" alt="The Actions wheel, with Copy lit"></td>
  </tr>
  <tr>
    <td><b>Shift: Convert.</b> Every format this file can become. The hub shows the result's size before you drop.</td>
    <td><b>Ctrl: Tools.</b> Compress, remove metadata, crop, edit, add a background, split, merge and more.</td>
    <td><b>Alt: Actions.</b> Share, Float, Copy, Move, Copy Path and open a Terminal there.</td>
  </tr>
</table>

1. **Start dragging** one or more files, from File Explorer, the desktop or any app.
2. **Tap a key once** while you hold the click. Tap another key to switch wheels.
3. **Drop on a petal.** Drop in the middle to cancel.

A small panel shows the progress and the result, and its **×** cancels the job:

<p align="center"><img src="docs/media/hud.png" width="420" alt="Converting to PNG: Saved Sunset.png"></p>

No drag handy? **Convert Files…** in Satsuma's menu opens the same wheel for files you pick; arrow
keys move around it, Enter applies.

## 🔁 What it converts

| | Convert to | Tools |
| --- | --- | --- |
| 🖼 **Images**: JPG, PNG, WebP, HEIC, AVIF, TIFF, BMP | each other, PDF or Word | Compress, Remove Metadata, Crop, Edit Photo, Add Background; with several: Create PDF, Collage |
| 🎬 **Video**: MP4, MOV, MKV, WebM, AVI, WMV | each other, GIF or MP3 | Compress, Remove Metadata, Mute, Speed, Snapshots, Split; with several: Join |
| 🎵 **Audio**: MP3, M4A, WAV, FLAC, OGG, Opus, AIFF, WMA | each other | Compress, Remove Metadata, Normalize, Trim, Mono / Stereo, Audio → Video |
| 📄 **PDF** | Word, JPG, PNG, EPUB, TXT, Markdown | Compress, Remove Metadata, Split; with several: Merge |
| 📝 **Documents**: Word, Markdown, RTF, HTML, TXT, EPUB | PDF, Word, TXT, HTML, EPUB, PNG, JPG | |
| 📊 **Spreadsheets**: Excel, ODS, CSV, TSV | each other or PDF | |
| 🗜 **Archives**: ZIP, TAR, GZ | each other | Extract |

Images, documents, PDFs, sheets, subtitles and archives work out of the box. Audio and video use
[ffmpeg](https://ffmpeg.org): `winget install Gyan.FFmpeg`.

## ✂️ Editors built in

<table>
  <tr>
    <td width="33%"><img src="docs/media/editor-crop.png" alt="Crop Image with aspect ratio presets and exact size fields"></td>
    <td width="33%"><img src="docs/media/editor-background.png" alt="Add Background with gradients, colours, padding, corners, shadow and ratio"></td>
    <td width="33%"><img src="docs/media/editor-edit.png" alt="Edit Photo with Light, Color, Detail and Effects adjustments"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Crop Image</b>: presets or an exact size</sub></td>
    <td align="center"><sub><b>Add Background</b>: gradients, colours or your own image</sub></td>
    <td align="center"><sub><b>Edit Photo</b>: 20 adjustments with a live preview</sub></td>
  </tr>
</table>

<img src="docs/media/float.png" align="right" width="110" alt="The Float shelf holding Sunset.jpg">

**Float** keeps files at hand in a small shelf that stays on top; drag them out again into any app
when you need them.

<br clear="right">

## 🎨 Liquid Glass

<img src="docs/media/settings-appearance.png" align="right" width="42%" alt="Settings: Appearance, with the wheel preview, Liquid and Solid themes and the transparency slider">

- **Liquid**: frosted glass that blurs whatever is behind it, with a highlight that flows like
  liquid from petal to petal as you move.
- **Solid**: an opaque wheel in Windows' light or dark colours.
- A **Transparency** slider for both, and a live preview to try them on.

<br clear="right">

## ⚙️ Make it yours

<table>
  <tr>
    <td width="33%"><img src="docs/media/settings-general.png" alt="Settings: General"></td>
    <td width="33%"><img src="docs/media/shortcuts.gif" alt="Settings: Shortcuts, with the animated how-to"></td>
    <td width="33%"><img src="docs/media/settings-files.png" alt="Settings: Files"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>General</b>: terminal app, Float, start at sign-in</sub></td>
    <td align="center"><sub><b>Shortcuts</b>: pick the key for each wheel</sub></td>
    <td align="center"><sub><b>Files</b>: compression strength, image size, video parts</sub></td>
  </tr>
</table>

## 🔄 Always up to date

Satsuma checks for new versions on its own and downloads them in the background. When one is
ready, the middle of the wheel says **Update ready**:

- **Drop your files there** and Satsuma updates and restarts right away, or
- do nothing: it updates itself the moment it's idle and comes back on the new version.

You can also check yourself in *Settings › About*.

## 📦 Install

<img src="docs/media/setup-welcome.png" align="right" width="50%" alt="Satsuma Setup">

1. Download **[Satsuma-Setup.exe](https://github.com/thesaikodelik/satsuma-windows/releases/latest/download/Satsuma-Setup.exe)**.
2. Run it and click **Install**. It installs just for you, **no administrator rights needed**.
3. Satsuma's citrus icon appears at the right end of the taskbar (it may be under the **^**).

> New apps can get *"Windows protected your PC"* until they're widely downloaded. Choose
> **More info → Run anyway**. Satsuma is open source: read every line, or build it yourself.

Uninstall from **Settings → Apps**. You can keep your settings.

<br clear="right">

## 🗂 What's new

The newest version, its files and **everything that changed in every version so far** are on the
**[latest release](https://github.com/thesaikodelik/satsuma-windows/releases/latest)**. You only ever
need that one; Satsuma updates itself after that.

## 🔒 Private by design

Your files never leave your PC. The only time Satsuma goes online is to ask GitHub whether a newer
version exists. No accounts, no ads, no tracking.

<details>
<summary><b>🛠 Build from source, command line and how it works</b></summary>

### Build

Stable Rust with the MSVC toolchain.

```powershell
winget install Rustlang.Rustup
cargo build --release -p satsuma-app -p satsuma-setup   # satsuma.exe, satsuma-cli.exe, satsuma-setup.exe
cargo test --workspace
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1   # dist\Satsuma-Setup-<version>.exe
```

### Command line

`satsuma-cli.exe help` lists every command; conversions, tools and capability checks all work from a
terminal.

### Layout

```
crates/
  satsuma-core/     formats, wheels, gestures and settings (shared with macOS)
  satsuma-engine/   conversions and tools; Windows services in src/win/
  satsuma-app/      the notification-area app, wheel, editors, Settings, updater and Setup (src/win/)
```

Windows has no shared drag pasteboard, so Satsuma lets the drag announce itself: when a wheel key is
tapped while a button is held, an invisible drop target appears under the pointer, and only a real
file drag reaches it. More in [docs/WINDOWS.md](docs/WINDOWS.md).

</details>

## 💛 Credits

- [**Satsuma**](https://github.com/sonatina-dream/satsuma) by **sonatina-dream**, the macOS app this
  edition is built from.
- The workflow is inspired by [Tangerine](https://tangerineformac.com) for Mac.
- Icons in the highlights: [Fluent Emoji](https://github.com/microsoft/fluentui-emoji) by Microsoft (MIT).
- Windows edition by [**thesaikodelik**](https://github.com/thesaikodelik). If it saves you time,
  you can [buy me a coffee](https://github.com/sponsors/thesaikodelik). ☕

Released under the [MIT license](LICENSE).
