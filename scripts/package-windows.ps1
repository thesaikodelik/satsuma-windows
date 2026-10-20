# Builds the Windows release and packs dist\Satsuma-<version>-windows-x64.zip
# (satsuma.exe, satsuma-cli.exe, README.txt, LICENSE).
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
cargo build --release -p satsuma-app
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$name = "Satsuma-$version-windows-x64"
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage | Out-Null

Copy-Item target\release\satsuma.exe, target\release\satsuma-cli.exe, LICENSE $stage
@"
Satsuma $version for Windows
============================

Drag files, tap a key, and a wheel of options opens under the pointer. Drop onto a
petal and the result is saved next to the original. Everything runs on your PC.

Install
-------
1. Move this folder somewhere permanent, for example
   %LOCALAPPDATA%\Programs\Satsuma
2. Run satsuma.exe. The first time, Windows SmartScreen may say it protected your PC
   (the app isn't signed): click "More info", then "Run anyway".
3. Satsuma's citrus icon appears in the notification area at the right end of the
   taskbar (it may be under the ^ arrow; drag it onto the taskbar to keep it visible).
   Turn on "Launch at Login" in its menu to start it with Windows.
4. For audio and video, install ffmpeg:  winget install Gyan.FFmpeg
   Images, documents, PDFs, spreadsheets, subtitles and archives work without it.

Use
---
While dragging files from File Explorer or the desktop, tap
  Shift  for conversions,  Ctrl  for tools,  Alt  for file actions.
Drop on a petal, or in the centre to cancel. Keys can be changed under Wheel Keys.
Without dragging: Convert Files... in Satsuma's menu, then arrow keys and Enter.

Command line: satsuma-cli.exe help

Settings: %APPDATA%\Satsuma\settings.json
"@ | Set-Content -Encoding UTF8 (Join-Path $stage 'README.txt')

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "Packed $zip"
