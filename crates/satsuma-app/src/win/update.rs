//! Self-update from GitHub Releases. A background thread checks the latest release now
//! and then; a newer `Satsuma-<version>-windows-x64.zip` is downloaded and unpacked into
//! `%LOCALAPPDATA%\Satsuma\Updates\<version>`. The wheel's hub says an update is ready,
//! and the next start (or Restart to Update in the tray menu) swaps the executables:
//! Windows lets a running .exe be renamed, so the old one moves aside as `*.old` and is
//! deleted on the following start.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use windows::core::HSTRING;

/// Where releases are published.
const REPO: &str = "thesaikodelik/satsuma-windows";
/// The executables a release replaces.
const EXES: [&str; 2] = ["satsuma.exe", "satsuma-cli.exe"];
/// The first check waits for the session to settle; later ones are rare.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// The downloaded version waiting to be installed.
static READY: Mutex<Option<String>> = Mutex::new(None);

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// `1.2.3` (or `v1.2.3`) as numbers; anything after a `-` is ignored.
fn parse(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// Whether `candidate` is a later version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

fn updates_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Satsuma").join("Updates"))
}

fn marker(dir: &Path) -> PathBuf {
    dir.join("ready.txt")
}

/// The version downloaded and ready to install, if any.
pub fn ready() -> Option<String> {
    READY.lock().ok()?.clone()
}

fn staged() -> Option<(String, PathBuf)> {
    let dir = updates_dir()?;
    let version = std::fs::read_to_string(marker(&dir)).ok()?.trim().to_string();
    let files = dir.join(&version);
    (is_newer(&version, CURRENT) && files.join(EXES[0]).is_file()).then_some((version, files))
}

/// Starts the background checks.
pub fn start() {
    if let Some((version, _)) = staged() {
        if let Ok(mut r) = READY.lock() {
            *r = Some(version);
        }
    }
    let _ = std::thread::Builder::new().name("update".into()).spawn(|| {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let first = std::env::var("SATSUMA_UPDATE_DELAY").ok().and_then(|s| s.parse().ok()).map(Duration::from_secs).unwrap_or(FIRST_CHECK);
        std::thread::sleep(first);
        loop {
            if let Err(e) = check() {
                debug!("update check: {e}");
            }
            std::thread::sleep(EVERY);
        }
    });
}

fn download(url: &str, to: &Path) -> Result<(), String> {
    use windows::Win32::System::Com::Urlmon::URLDownloadToFileW;
    let _ = std::fs::remove_file(to);
    unsafe { URLDownloadToFileW(None, &HSTRING::from(url), &HSTRING::from(to.as_os_str()), 0, None) }.map_err(|e| format!("download {url}: {e}"))?;
    if to.is_file() {
        Ok(())
    } else {
        Err(format!("download {url}: nothing saved"))
    }
}

/// Looks at the latest release; downloads and unpacks it when it is newer.
fn check() -> Result<(), String> {
    let dir = updates_dir().ok_or("no LOCALAPPDATA")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let api = std::env::var("SATSUMA_UPDATE_URL").unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"));
    // A changing query keeps the system's download cache from answering with an old copy.
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let json_path = dir.join("latest.json");
    download(&format!("{api}{}t={stamp}", if api.contains('?') { '&' } else { '?' }), &json_path)?;
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&json_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&json_path);
    let tag = json["tag_name"].as_str().ok_or("no tag_name")?;
    // Tags look like `v0.1.2-windows`; the version is the numbers.
    let version = tag.trim_start_matches(['v', 'V']).split(['-', '+']).next().unwrap_or("").to_string();
    if !is_newer(&version, CURRENT) || ready().as_deref() == Some(version.as_str()) {
        return Ok(());
    }
    let asset = json["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str().is_some_and(|n| n.ends_with("windows-x64.zip"))))
        .and_then(|x| x["browser_download_url"].as_str())
        .ok_or("no Windows asset")?;
    debug!("update {version} available; downloading");
    let zip_path = dir.join(format!("{version}.zip"));
    download(asset, &zip_path)?;
    let target = dir.join(&version);
    let unpacked = unpack(&zip_path, &target);
    let _ = std::fs::remove_file(&zip_path);
    unpacked?;
    std::fs::write(marker(&dir), &version).map_err(|e| e.to_string())?;
    // Older downloads are no longer needed.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().is_dir() && e.file_name() != version.as_str() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    debug!("update {version} ready");
    if let Ok(mut r) = READY.lock() {
        *r = Some(version);
    }
    Ok(())
}

/// Takes the executables out of the release zip (they sit in a versioned folder).
fn unpack(zip_path: &Path, target: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(target);
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = entry.name().map_err(|e| e.to_string())?.to_string();
        let name = path.rsplit(['/', '\\']).next().unwrap_or("").to_ascii_lowercase();
        if EXES.contains(&name.as_str()) {
            let mut out = std::fs::File::create(target.join(&name)).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    if target.join(EXES[0]).is_file() {
        Ok(())
    } else {
        Err("the release has no satsuma.exe".into())
    }
}

fn old_of(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// Deletes executables left aside by the last update.
pub fn clean_up() {
    let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else { return };
    for name in EXES {
        let _ = std::fs::remove_file(old_of(&dir.join(name)));
    }
}

/// Puts the downloaded executables in place of the running ones. On success the caller
/// starts the new `satsuma.exe` and quits.
pub fn install() -> Result<String, String> {
    let (version, files) = staged().ok_or("no update downloaded")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no folder")?.to_path_buf();
    let mut moved: Vec<PathBuf> = Vec::new();
    let mut result = Ok(());
    for name in EXES {
        let (from, to) = (files.join(name), dir.join(name));
        if !from.is_file() {
            continue;
        }
        if to.exists() {
            let old = old_of(&to);
            let _ = std::fs::remove_file(&old);
            if let Err(e) = std::fs::rename(&to, &old) {
                result = Err(format!("Couldn't replace {name}: {e}"));
                break;
            }
            moved.push(to.clone());
        }
        if let Err(e) = std::fs::copy(&from, &to) {
            result = Err(format!("Couldn't copy {name}: {e}"));
            break;
        }
    }
    if let Err(e) = result {
        // Put everything back as it was.
        for to in moved {
            let _ = std::fs::remove_file(&to);
            let _ = std::fs::rename(old_of(&to), &to);
        }
        return Err(e);
    }
    if let Some(updates) = updates_dir() {
        let _ = std::fs::remove_file(marker(&updates));
        let _ = std::fs::remove_dir_all(&files);
    }
    if let Ok(mut r) = READY.lock() {
        *r = None;
    }
    Ok(version)
}

/// Starts the (new) app; `--updated` makes it wait for this one to quit.
pub fn relaunch() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    std::process::Command::new(exe).arg("--updated").spawn().is_ok()
}

/// Launched by [`relaunch`].
pub fn after_update() -> bool {
    std::env::args().any(|a| a == "--updated")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert!(is_newer("0.2.0-beta", "0.1.0"));
    }
}
