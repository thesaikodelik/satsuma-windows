//! Getting ffmpeg for the user: the "essentials" build by Gyan Doshi, from its GitHub
//! releases (GyanD/codexffmpeg), checked against the SHA-256 GitHub publishes for the file.
//! The 7z package (about 35 MB) is unpacked with Windows' own tar; Windows versions whose
//! tar can't read 7z get the zip (about 115 MB) instead. Only ffmpeg.exe and its licence are
//! kept, in `%LOCALAPPDATA%\Satsuma\ffmpeg\<version>`.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const RELEASES: &str = "https://api.github.com/repos/GyanD/codexffmpeg/releases/latest";

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    /// Bytes so far and in all (0 while unknown).
    Downloading { done: u64, total: u64 },
    Unpacking,
    /// Installed: its version.
    Ready(String),
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or(State::Idle)
}

fn set(s: State) {
    if let Ok(mut st) = STATE.lock() {
        *st = s;
    }
}

pub fn busy() -> bool {
    matches!(state(), State::Downloading { .. } | State::Unpacking)
}

/// Acknowledges a finished or failed download, so the state goes back to idle.
pub fn reset() {
    if !busy() {
        set(State::Idle);
    }
}

pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Starts getting the latest ffmpeg in the background, unless that is already happening.
pub fn start() {
    if busy() {
        return;
    }
    CANCEL.store(false, Ordering::SeqCst);
    set(State::Downloading { done: 0, total: 0 });
    std::thread::spawn(|| match get(None) {
        Ok(version) => set(State::Ready(version)),
        Err(e) if CANCEL.load(Ordering::SeqCst) => {
            debug!("ffmpeg download cancelled: {e}");
            set(State::Idle)
        }
        Err(e) => {
            debug!("ffmpeg download failed: {e}");
            set(State::Failed(e))
        }
    });
}

/// Brings an ffmpeg Satsuma downloaded earlier up to date, quietly (from the update check).
pub fn refresh_quietly() {
    let Some((have, _)) = satsuma_engine::env::managed_ffmpeg() else { return };
    if busy() {
        return;
    }
    match get(Some(&have)) {
        Ok(v) if v != have => debug!("ffmpeg updated from {have} to {v}"),
        Ok(_) => {}
        Err(e) => debug!("ffmpeg update check: {e}"),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(20)).user_agent("Satsuma for Windows").build()
}

/// Downloads and installs the latest ffmpeg; with `have`, does nothing when that's the latest.
fn get(have: Option<&str>) -> Result<String, String> {
    let release: serde_json::Value = agent().get(RELEASES).call().map_err(|e| format!("Couldn't reach GitHub ({e}).")).and_then(|r| r.into_json().map_err(|e| e.to_string()))?;
    let version = release["tag_name"].as_str().ok_or("GitHub sent no version.")?.to_string();
    if have == Some(version.as_str()) {
        return Ok(version);
    }
    let assets = release["assets"].as_array().ok_or("The release has no files.")?;
    let asset = |suffix: &str| {
        assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.ends_with(suffix))).and_then(|a| {
            let url = a["browser_download_url"].as_str()?;
            let sha = a["digest"].as_str()?.strip_prefix("sha256:")?.to_ascii_lowercase();
            Some((url.to_string(), sha, a["size"].as_u64().unwrap_or(0)))
        })
    };
    let dir = satsuma_engine::env::managed_ffmpeg_dir().ok_or("There's no folder for local app data.")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(&version);
    let quiet = have.is_some();

    let mut installed = false;
    if tar_reads_7z() {
        if let Some((url, sha, size)) = asset("essentials_build.7z") {
            let pkg = dir.join("download.7z");
            fetch(&url, &pkg, &sha, size, quiet)?;
            if !quiet {
                set(State::Unpacking);
            }
            installed = unpack_7z(&pkg, &target).is_ok();
            let _ = std::fs::remove_file(&pkg);
        }
    }
    if !installed {
        let (url, sha, size) = asset("essentials_build.zip").ok_or("The release has no essentials build.")?;
        let pkg = dir.join("download.zip");
        fetch(&url, &pkg, &sha, size, quiet)?;
        if !quiet {
            set(State::Unpacking);
        }
        let unpacked = unpack_zip(&pkg, &target);
        let _ = std::fs::remove_file(&pkg);
        unpacked?;
    }
    // It must run before it's used.
    let mut check = std::process::Command::new(target.join("ffmpeg.exe"));
    check.arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let ok = satsuma_engine::process::hide_console(&mut check).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_dir_all(&target);
        return Err("The downloaded ffmpeg didn't start. Windows security settings (Smart App Control) may block it; install it with `winget install Gyan.FFmpeg` instead.".into());
    }
    std::fs::write(dir.join("current"), &version).map_err(|e| e.to_string())?;
    // A first download is used right away; an update replaces the old copy at the next start
    // (jobs may be running it now).
    if !quiet {
        clean_up();
    }
    Ok(version)
}

/// Removes versions other than the current one (at start, when none is running).
pub fn clean_up() {
    let Some(dir) = satsuma_engine::env::managed_ffmpeg_dir() else { return };
    let Ok(current) = std::fs::read_to_string(dir.join("current")) else { return };
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().is_dir() && e.file_name() != current.trim() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

/// Downloads `url` to `to`, reporting progress, and checks its SHA-256.
fn fetch(url: &str, to: &Path, sha: &str, size: u64, quiet: bool) -> Result<(), String> {
    let resp = agent().get(url).call().map_err(|e| format!("The download failed ({e})."))?;
    let total = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(size);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        if CANCEL.load(Ordering::SeqCst) && !quiet {
            drop(file);
            let _ = std::fs::remove_file(to);
            return Err("Cancelled.".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("The download stopped ({e})."))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hash.update(&buf[..n]);
        done += n as u64;
        if !quiet {
            set(State::Downloading { done, total });
        }
    }
    drop(file);
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != sha {
        let _ = std::fs::remove_file(to);
        return Err("The download didn't match the file GitHub published, so it wasn't used.".into());
    }
    Ok(())
}

fn system_tar() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join(r"System32\tar.exe")
}

/// Windows 11's tar (libarchive 3.7) reads 7z; Windows 10's doesn't.
fn tar_reads_7z() -> bool {
    let mut cmd = std::process::Command::new(system_tar());
    cmd.arg("--version");
    satsuma_engine::process::capture(&mut cmd)
        .ok()
        .and_then(|(_, out, _)| {
            let v = out.split("libarchive ").nth(1)?.split_whitespace().next()?.to_string();
            let mut it = v.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
            Some((it.next().unwrap_or(0), it.next().unwrap_or(0)) >= (3, 7))
        })
        .unwrap_or(false)
}

fn place(found: &[PathBuf], target: &Path) -> Result<(), String> {
    let exe = found.iter().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("ffmpeg.exe"))).ok_or("The package has no ffmpeg.exe.")?;
    let _ = std::fs::remove_dir_all(target);
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    std::fs::copy(exe, target.join("ffmpeg.exe")).map_err(|e| e.to_string())?;
    if let Some(lic) = found.iter().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("LICENSE"))) {
        let _ = std::fs::copy(lic, target.join("LICENSE.txt"));
    }
    Ok(())
}

fn unpack_7z(pkg: &Path, target: &Path) -> Result<(), String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(system_tar());
    cmd.arg("-xf").arg(pkg).arg("-C").arg(tmp.path()).args(["--include", "*/bin/ffmpeg.exe", "--include", "*/LICENSE"]);
    let (ok, _, err) = satsuma_engine::process::capture(&mut cmd).map_err(|e| e.to_string())?;
    if !ok {
        return Err(format!("tar couldn't unpack ffmpeg: {}", err.trim()));
    }
    let found: Vec<PathBuf> = walk(tmp.path());
    place(&found, target)
}

fn unpack_zip(pkg: &Path, target: &Path) -> Result<(), String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pkg).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.replace('\\', "/");
        let keep = name.ends_with("/bin/ffmpeg.exe") || name.rsplit('/').next() == Some("LICENSE");
        if keep {
            let out = tmp.path().join(name.rsplit('/').next().unwrap_or("file"));
            let mut f = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
            found.push(out);
        }
    }
    place(&found, target)
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// "12.3 of 35 MB".
pub fn progress_text(done: u64, total: u64) -> String {
    let mb = |b: u64| b as f64 / 1_048_576.0;
    if total > 0 {
        format!("{:.1} of {:.0} MB", mb(done), mb(total))
    } else {
        format!("{:.1} MB", mb(done))
    }
}
