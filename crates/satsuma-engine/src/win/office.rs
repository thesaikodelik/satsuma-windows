//! Presentations (PPT, PPTX) to PDF through Microsoft PowerPoint when it's installed, or
//! LibreOffice otherwise. Satsuma can't read slides itself.

use crate::process::{capture, Cancel};
use crate::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

fn powerpoint() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CLASSES_ROOT, KEY_READ};
    let mut key = HKEY::default();
    let found = unsafe { RegOpenKeyExW(HKEY_CLASSES_ROOT, w!("PowerPoint.Application\\CLSID"), None, KEY_READ, &mut key) }.is_ok();
    if found {
        unsafe {
            let _ = RegCloseKey(key);
        }
    }
    found
}

fn soffice() -> Option<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|v| std::env::var_os(v))
        .map(|d| PathBuf::from(d).join("LibreOffice").join("program").join("soffice.exe"))
        .find(|p| p.is_file())
}

/// Whether slides can be turned into PDFs here.
pub fn can_convert_slides() -> bool {
    powerpoint() || soffice().is_some()
}

fn powershell() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe")
}

/// PowerPoint, invisibly: open read-only, save as PDF, close. PowerPoint quits afterwards
/// only when nothing else is open in it, so the user's own presentations stay put.
const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$app = New-Object -ComObject PowerPoint.Application
try {
  $deck = $app.Presentations.Open($env:SATSUMA_IN, -1, 0, 0)
  try { $deck.SaveAs($env:SATSUMA_OUT, 32) } finally { $deck.Close() }
} finally {
  if ($app.Presentations.Count -eq 0) { $app.Quit() }
  [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($app)
}
"#;

pub fn slides_to_pdf(input: &Path, out: &Path, tmp: &Path, cancel: &Cancel) -> Result<()> {
    cancel.check()?;
    // One conversion at a time: PowerPoint is a single shared app, and two scripts opening
    // decks in it at once make one fail.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    cancel.check()?;
    if powerpoint() {
        let mut cmd = Command::new(powershell());
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT]);
        cmd.env("SATSUMA_IN", input).env("SATSUMA_OUT", out);
        let (ok, _, err) = capture(&mut cmd)?;
        if ok && out.is_file() {
            return Ok(());
        }
        if soffice().is_none() {
            let reason = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            return Err(Error::msg(format!("PowerPoint couldn't save the slides as a PDF. {reason}").trim().to_string()));
        }
    }
    let Some(office) = soffice() else {
        return Err(Error::msg("Converting slides needs Microsoft PowerPoint or LibreOffice."));
    };
    let dir = tmp.join("slides");
    std::fs::create_dir_all(&dir)?;
    // A private profile, so a LibreOffice the user has open doesn't block the conversion.
    let profile = format!("-env:UserInstallation=file:///{}", tmp.join("lo-profile").to_string_lossy().replace('\\', "/"));
    let mut cmd = Command::new(office);
    cmd.arg(profile).args(["--headless", "--convert-to", "pdf", "--outdir"]).arg(&dir).arg(input);
    let (_, _, err) = capture(&mut cmd)?;
    let made = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")));
    match made {
        Some(pdf) => {
            std::fs::copy(&pdf, out)?;
            Ok(())
        }
        None => Err(Error::msg(format!("LibreOffice couldn't convert the slides. {}", err.trim()).trim().to_string())),
    }
}
