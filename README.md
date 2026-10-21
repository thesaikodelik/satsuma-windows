<p align="center">
  <img src="docs/media/hero.jpg" alt="Satsuma: drag a file, tap a key, drop it on a petal. A glass wheel of formats opens under the pointer and the photo is saved as PNG." width="100%">
</p>

<p align="center">
  <a href="https://github.com/thesaikodelik/satsuma-windows/releases/latest/download/Satsuma-Setup.exe"><img src="https://img.shields.io/badge/download-Satsuma--Setup.exe-ff6e17?style=for-the-badge&logo=windows11&logoColor=white" alt="Download Satsuma for Windows"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Windows-11-0078d4?logo=windows11&logoColor=white" alt="Windows 11">
  <img src="https://img.shields.io/badge/version-1.1.0-ff6e17" alt="Version 1.1.0">
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
    <td width="33%" align="center"><img src="docs/media/wheel-tools.png" alt="The Tools wheel for a photo, with Annotate lit"></td>
    <td width="33%" align="center"><img src="docs/media/wheel-actions.png" alt="The Actions wheel, with Copy lit"></td>
  </tr>
  <tr>
    <td><b>Shift: Convert.</b> Every format this file can become. The hub shows the result's size before you drop.</td>
    <td><b>Ctrl: Tools.</b> Compress, trim, crop, edit, annotate, redact, organize pages, edit tags and more.</td>
    <td><b>Alt: Actions.</b> Share, Float, Copy, Move, Copy Path and open a Terminal there.</td>
  </tr>
</table>

1. **Start dragging** one or more files, from File Explorer, the desktop or any app.
2. **Tap a key once** while you hold the click. Tap another key to switch wheels.
3. **Drop on a petal.** Drop in the middle to cancel.

<img src="docs/media/wheel-progress.png" align="right" width="230" alt="The wheel converting a video to WebM: a thin orange ring of light fills along its edge and the hub shows 45%">

**The wheel shows the work.** After you drop, the wheel stays where it is: a thin ring of light
fills along its edge, and the hub counts the percent. Hover the hub to **Cancel**; a tick shows when
it's done. A small panel reports the result:

<img src="docs/media/hud.png" width="380" alt="Converting to PNG: Saved Sunset.png">

No drag handy? **Convert Files…** in Satsuma's menu opens the same wheel for files you pick; arrow
keys move around it, Enter applies.

<br clear="right">

## 🔁 What it converts

| | Convert to |
| --- | --- |
| 🖼 **Images**: JPG, PNG, WebP, HEIC, AVIF, TIFF, BMP, SVG | each other, PDF or Word (text is recognised) |
| 🎬 **Video**: MP4, MOV, MKV, WebM, AVI, WMV | each other, GIF or MP3 |
| 🎵 **Audio**: MP3, M4A, WAV, FLAC, OGG, Opus, AIFF, WMA | each other |
| 📄 **PDF** | Word, **PowerPoint**, **Excel**, JPG, PNG, EPUB, TXT, Markdown |
| 📽 **Slides**: PowerPoint (PPT, PPTX) | PDF, JPG, PNG |
| 📝 **Documents**: Word, Markdown, RTF, HTML, TXT, EPUB | PDF, Word, TXT, HTML, EPUB, PNG, JPG |
| 📊 **Spreadsheets**: Excel, ODS, CSV, TSV | each other or PDF |
| 🗜 **Archives**: ZIP, TAR, GZ | each other |

<img src="docs/media/wheel-convert-pdf.png" align="right" width="230" alt="The Convert wheel for a PDF, with PPTX lit and its size, 1.3 MB, in the hub">

- **PDF → PowerPoint** puts each page on its own slide.
- **PDF → Excel** reads the pages, scanned ones too, and lays the text out in rows and columns.
- **Slides → PDF** uses Microsoft PowerPoint, or LibreOffice when there's no PowerPoint.

Images, documents, PDFs, sheets, subtitles and archives work out of the box. Audio and video use
[ffmpeg](https://ffmpeg.org): `winget install Gyan.FFmpeg`.

<br clear="right">

## 🧰 Tools

Many tools open a small window first, so you choose exactly what happens. Everything below runs on
your PC.

### 🖼 Photos

<table>
  <tr>
    <td width="50%"><img src="docs/media/tool-annotate.png" alt="Annotate Photo: an arrow, an ellipse, two numbered markers and the words Swim here! on a lake"></td>
    <td width="50%"><img src="docs/media/tool-redact-photo.png" alt="Redact Photo: the Wi-Fi password on a cabin card is blurred, the network name is not"></td>
  </tr>
  <tr>
    <td><b>Annotate</b>: arrows, boxes, ellipses, lines, highlights, text and numbered markers, in seven colours and three sizes.</td>
    <td><b>Redact</b>: blur, pixelate or black out what you mark. <b>Find Text</b> and <b>Find Faces</b> mark them for you.</td>
  </tr>
</table>

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

Also: **Compress** (to a strength or a size, with a longest edge), **Remove Metadata** (camera and
GPS details), and for several photos **Create PDF** and **Collage** (grid, row or column, with
spacing and a background colour).

### 🎬 Video

<table>
  <tr>
    <td width="33%"><img src="docs/media/tool-trim-video.png" alt="Trim Video: start and end handles on a filmstrip of lake scenes, keeping 0:03 to 0:14"></td>
    <td width="33%"><img src="docs/media/tool-crop-video.png" alt="Crop Video: a 9:16 box over a lake scene"></td>
    <td width="33%"><img src="docs/media/tool-redact-video.png" alt="Redact Video: the sun pixelated from 0:04 to 0:14"></td>
  </tr>
  <tr>
    <td><b>Trim</b>: drag the handles on the filmstrip, with a large preview, or type exact times.</td>
    <td><b>Crop</b>: drag the box, or pick 16:9, 9:16, 1:1, 4:3 or 4:5.</td>
    <td><b>Redact</b>: blur, pixelate or black out boxes, for the whole clip or just part of it.</td>
  </tr>
</table>

<table>
  <tr>
    <td width="50%"><img src="docs/media/tool-speed.png" alt="Change Video Speed: 2× with the pitch kept; 0:20 becomes 0:10"></td>
    <td width="50%"><img src="docs/media/tool-compress.png" alt="Compress to a size: 5.6 MB now, about 2.8 MB after"></td>
  </tr>
  <tr>
    <td><b>Speed</b>: 0.25× to 4×, keeping the pitch of the voice if you like.</td>
    <td><b>Compress</b>: Balanced, Strong, or <b>to a size</b> that fits where you're sending it.</td>
  </tr>
</table>

Also: **Snapshots** (1 to 30 frames spread through the clip, PNG or JPG), **Split** (into equal
parts or clips of a set length), **Mute**, **Edit Metadata**, and for several clips **Join**.

### 🎵 Audio

<table>
  <tr>
    <td width="50%"><img src="docs/media/tool-bleep.png" alt="Bleep Audio: two parts of a voice note marked on the waveform, where a phone number is said"></td>
    <td width="50%"><img src="docs/media/tool-metadata.png" alt="Edit Metadata: title, artist, album, year, genre and comment of a voice note"></td>
  </tr>
  <tr>
    <td><b>Bleep</b>: mark parts on the waveform; they get a beep or silence.</td>
    <td><b>Edit Metadata</b>: title, artist, album, year, genre and comment, or remove everything.</td>
  </tr>
</table>

<img src="docs/media/tool-visualize.png" align="right" width="46%" alt="Audio to Video: landscape, portrait or square, with a moving or still waveform">

Also: **Trim** on the waveform (or just cut the silence at both ends), **Mono / Stereo** with the
volume of each side, **Normalize**, **Compress** and **Audio → Video** (landscape, portrait or
square, with a moving or still waveform).

<br clear="right">

### 📄 PDF

<img src="docs/media/tool-organize.png" align="right" width="46%" alt="Organize PDF: six journal pages as thumbnails, page 6 moved up and page 4 turned 90°">

**Organize Pages**: every page as a thumbnail. Drag pages into a new order, turn them, duplicate or
delete them, then save.

Also: **Compress**, **Split**, **Edit Info** (title, author, subject and keywords), and for several
PDFs **Merge**.

<br clear="right">

### 📌 Float

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
