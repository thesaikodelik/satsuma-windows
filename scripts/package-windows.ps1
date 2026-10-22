# Builds the Windows release and packs dist\Satsuma-<version>-windows-x64.zip
# (satsuma.exe, satsuma-cli.exe, README.txt, LICENSE).
#
#   powershell -ExecutionPolicy Bypass -File scripts\package-windows.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
# Keep the builder's home folder (and so their user name) out of the executables' embedded paths.
$env:RUSTFLAGS = "--remap-path-prefix=$($env:USERPROFILE)=~"
cargo build --release -p satsuma-app -p satsuma-setup
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
   Turn on "Launch at login" in Settings > General to start it with Windows.
4. Audio and video need ffmpeg. Satsuma gets it for you: drag an audio or video file,
   tap a wheel key and drop it on "Get ffmpeg" in the middle (or Settings > General).
   Images, documents, PDFs, spreadsheets, subtitles and archives work without it.

Use
---
While dragging files from File Explorer or the desktop, tap
  Shift  for conversions,  Ctrl  for tools,  Alt  for file actions.
Drop on a petal, or in the centre to cancel. Keys, theme and transparency are in Settings.
Without dragging: Convert Files... in Satsuma's menu, then arrow keys and Enter.

Updates
-------
Satsuma checks for new versions itself and downloads them in the background. When one
is ready, the middle of the wheel says "Update ready"; it installs the next time
Satsuma starts, or right away with "Restart to Update" in its menu.

Command line: satsuma-cli.exe help

Settings: %APPDATA%\Satsuma\settings.json
"@ | Set-Content -Encoding UTF8 (Join-Path $stage 'README.txt')

$zip = Join-Path $dist "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "Packed $zip"

# Satsuma Setup: the installer program with the release zip appended
# (stub | zip | zip length as u64 LE | "SATSUMA1").
$setup = Join-Path $dist 'Satsuma-Setup.exe'
$stub = [IO.File]::ReadAllBytes((Join-Path $root 'target\release\satsuma-setup.exe'))
$payload = [IO.File]::ReadAllBytes($zip)
$out = New-Object IO.MemoryStream
$out.Write($stub, 0, $stub.Length)
$out.Write($payload, 0, $payload.Length)
$len = [BitConverter]::GetBytes([uint64]$payload.Length)
$out.Write($len, 0, 8)
$magic = [Text.Encoding]::ASCII.GetBytes('SATSUMA1')
$out.Write($magic, 0, 8)
[IO.File]::WriteAllBytes($setup, $out.ToArray())
Write-Host "Packed $setup"
# Release: Satsuma-Setup.exe (a fixed name, so releases/latest/download/Satsuma-Setup.exe always
# works) and the zip, which the app's updater downloads.
Get-FileHash $setup, $zip -Algorithm SHA256 | ForEach-Object { '{0}  {1}' -f $_.Hash.ToLower(), (Split-Path $_.Path -Leaf) }
