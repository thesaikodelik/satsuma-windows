# Satsuma on Windows

The Windows version has the same gesture, wheels, format matrix, naming rules, tools, actions and
editors as the macOS app. `satsuma-core` is shared unchanged. The engine and the app swap each macOS
system service for a Windows one behind the same interfaces.

Requirements: Windows 10 (1903) or later, x64. Optional: ffmpeg (`winget install Gyan.FFmpeg`) for
audio and video.

## Using it

- The app lives in the **notification area** (the citrus icon at the right end of the taskbar).
  Click it for the menu.
- While dragging files, tap **Shift** to convert, **Ctrl** for tools and **Alt** for file actions.
  Change them under *Wheel Keys*. The Windows key isn't offered, because tapping it opens Start.
- **AirDrop** becomes **Share**, the Windows share sheet (Nearby Sharing, Mail, Phone Link and so on).
- **Terminal** opens Windows Terminal, PowerShell or Command Prompt (*Terminal App* in the menu).
- *Launch at Login* writes `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Satsuma`.
- Settings: `%APPDATA%\Satsuma\settings.json`.
- `satsuma-cli.exe` is the command-line build, with the same commands as on macOS.
  `satsuma.exe <command>` works too.

## How the drag gesture works without a drag pasteboard

macOS exposes a shared drag pasteboard whose change count rises when any app starts a drag. Windows
has no equivalent, and global hooks would need more privileges than the app should have. So Satsuma
lets the drag announce itself:

1. A 60 Hz timer samples the primary mouse button, the modifier keys (`GetAsyncKeyState`) and the
   pointer position. This needs no Accessibility-style permission.
2. When a wheel key is tapped while the button is held (or held while the pointer leaves the press
   point), the wheel window appears under the pointer as a **probe**. It is a topmost, non-activating
   layered window at alpha 1/255, so it is invisible but still hit-testable, and it is registered with
   `RegisterDragDrop`.
3. If an OLE drag is running, the drag loop calls the probe's `IDropTarget::DragEnter`. A data
   object with `CF_HDROP` confirms a file drag, and the wheel is drawn in place. If no drag reaches the
   probe within 650 ms, or the drag carries no files (text, browser images), the probe disappears and
   nothing is shown.
4. `DragOver` hit-tests the petals and returns Copy or None. `Drop` on a petal starts the job, and a
   drop off a petal is refused, so the drag is cancelled. When the button comes up, the wheel closes.

## macOS → Windows

| macOS | Windows |
|-------|---------|
| Menu-bar status item, `LSUIElement` | `Shell_NotifyIcon` and a popup menu; hidden main window |
| `NSPasteboard(.drag).changeCount` | Probe window plus `IDropTarget::DragEnter` (above) |
| `NSPanel` wheel and HUD, AppKit drawing | Layered windows (`UpdateLayeredWindow`) drawn with Direct2D/DirectWrite |
| SF Symbols | Segoe Fluent Icons (Segoe MDL2 Assets on Windows 10) |
| `NSDraggingSource` (Float) | `DoDragDrop` with the shell's data object (`BHID_DataObject`) |
| `sips` (ImageIO) | `image` crate, `resvg` (SVG), WIC (HEIC, AVIF, camera formats), ffmpeg (WebP and AVIF out) |
| CoreGraphics compositing (Add Background) | tiny-skia |
| CoreImage (Edit Photo) | Same filter chain on float pixels, in Rust |
| PDFKit (text, merge, split, metadata, compress) | lopdf, pdf-extract |
| CoreGraphics PDF rendering | `Windows.Data.Pdf` |
| `textutil` + CoreText pagination | DOCX/RTF readers in Rust; HTML → PDF printed by headless Edge |
| Vision OCR | `Windows.Media.Ocr` |
| `/usr/bin/tar` | `%SystemRoot%\System32\tar.exe` (bsdtar) |
| VideoToolbox encoders | libx264/libx265, or Media Foundation (`h264_mf`, `hevc_mf`) |
| `SMAppService` | `HKCU\…\Run` |

## What depends on the PC

Petals whose backend is missing are hidden, as on macOS.

- **HEIC output** needs the *HEIF Image Extensions* and *HEVC Video Extensions* from the Microsoft
  Store. HEIC input also works through ffmpeg.
- **AVIF output** needs ffmpeg with an AV1 encoder (libaom, SVT-AV1 or rav1e).
- **Documents → PDF/PNG/JPG** use Microsoft Edge, which ships with Windows.
- **OCR** (image → DOCX, scanned PDFs) uses the Windows OCR languages for your display languages.
  Add one under *Settings › Time & language › Language & region* if recognition is unavailable.

## Differences from the macOS app

- There is no haptic tick; the highlight sound remains (*Sound Feedback*).
- *Convert Files…* picks files only: the Windows file dialog can't select files and folders at once.
- The wheel's disc is semi-opaque instead of frosted glass. Acrylic doesn't combine with per-pixel
  alpha windows.
- The app isn't code-signed, so SmartScreen asks once.

## Development

```powershell
cargo test --workspace                     # core, engine and end-to-end conversion tests
cargo run -p satsuma-app --bin satsuma     # start the app
$env:SATSUMA_DEBUG_LOG="$env:TEMP\satsuma.log"; cargo run -p satsuma-app --bin satsuma   # log gestures and jobs
cargo run -p satsuma-app --bin satsuma-cli -- render-wheel --mode tools --format jpg --out wheel.png
powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1   # dist\Satsuma-<version>-windows-x64.zip
```

Debug builds have two more commands for checking the real gesture:

- `satsuma-cli selftest-drag [--key shift|ctrl|alt] [--petal N] [--center] [--screenshot out.png] <files…>`
  starts a real OLE drag from its own small window. It then taps the key, glides to petal *N* of the
  running app's wheel and drops, using `SendInput`. It only releases over the wheel; anywhere else it
  presses Escape.
- `satsuma-cli selftest-screenshot "<window title>" out.png` captures a window.
