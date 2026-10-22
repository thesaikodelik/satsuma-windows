//! Discovery of the external tools Satsuma relies on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct Env {
    /// ffmpeg, if installed (audio, video, WebP, collages, PNG compression).
    pub ffmpeg: Option<PathBuf>,
    encoders: HashSet<String>,
    #[cfg(target_os = "macos")]
    pub sips: PathBuf,
    #[cfg(target_os = "macos")]
    pub textutil: PathBuf,
    /// bsdtar: `/usr/bin/tar` on macOS, `%SystemRoot%\System32\tar.exe` on Windows 10 and later.
    pub tar: PathBuf,
    /// Microsoft Edge (or Chrome), used on Windows to print HTML documents to PDF.
    #[cfg(windows)]
    pub browser: Option<PathBuf>,
}

impl Env {
    /// Finds tools in `$SATSUMA_FFMPEG`, next to the app, in package-manager folders and on `PATH`.
    pub fn discover() -> Env {
        let ffmpeg = find_ffmpeg();
        let encoders = ffmpeg.as_deref().map(list_encoders).unwrap_or_default();
        #[cfg(target_os = "macos")]
        return Env {
            ffmpeg,
            encoders,
            sips: PathBuf::from("/usr/bin/sips"),
            textutil: PathBuf::from("/usr/bin/textutil"),
            tar: PathBuf::from("/usr/bin/tar"),
        };
        #[cfg(windows)]
        return Env { ffmpeg, encoders, tar: system32("tar.exe"), browser: find_browser() };
    }

    /// An environment without ffmpeg (for tests and diagnostics).
    pub fn without_ffmpeg() -> Env {
        Env { ffmpeg: None, encoders: HashSet::new(), ..Env::discover() }
    }

    pub fn has_ffmpeg(&self) -> bool {
        self.ffmpeg.is_some()
    }

    pub fn has_encoder(&self, name: &str) -> bool {
        self.ffmpeg.is_some() && self.encoders.contains(name)
    }

    /// The first available encoder of `names`.
    pub fn pick_encoder<'a>(&self, names: &[&'a str]) -> Option<&'a str> {
        names.iter().copied().find(|n| self.has_encoder(n))
    }

    pub fn ffmpeg_command(&self) -> Option<Command> {
        let mut cmd = Command::new(self.ffmpeg.as_ref()?);
        cmd.args(["-hide_banner", "-nostdin", "-y"]);
        Some(cmd)
    }
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(windows)]
fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
    root.join("System32").join(name)
}

/// Edge ships with Windows 10 and 11; Chrome works the same way.
#[cfg(windows)]
fn find_browser() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("SATSUMA_BROWSER") {
        candidates.push(PathBuf::from(p));
    }
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(dir) = std::env::var_os(var).map(PathBuf::from) {
            candidates.push(dir.join(r"Microsoft\Edge\Application\msedge.exe"));
            candidates.push(dir.join(r"Google\Chrome\Application\chrome.exe"));
        }
    }
    candidates.into_iter().find(|p| is_executable(p))
}

/// Where Satsuma keeps the ffmpeg it downloads itself: `%LOCALAPPDATA%\Satsumafmpeg`, one
/// folder per version, with `current` naming the one in use.
#[cfg(windows)]
pub fn managed_ffmpeg_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Satsuma").join("ffmpeg"))
}

/// The ffmpeg Satsuma downloaded, and its version, if there is one.
#[cfg(windows)]
pub fn managed_ffmpeg() -> Option<(String, PathBuf)> {
    let dir = managed_ffmpeg_dir()?;
    let version = std::fs::read_to_string(dir.join("current")).ok()?.trim().to_string();
    let exe = dir.join(&version).join("ffmpeg.exe");
    (!version.is_empty() && is_executable(&exe)).then_some((version, exe))
}

#[cfg(windows)]
fn find_ffmpeg() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("SATSUMA_FFMPEG") {
        candidates.push(PathBuf::from(p));
    }
    if let Some((_, exe)) = managed_ffmpeg() {
        candidates.push(exe);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ffmpeg.exe"));
            candidates.push(dir.join(r"bin\ffmpeg.exe"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join("ffmpeg.exe")));
    }
    // winget, Scoop and Chocolatey shims, and the usual manual install folder.
    if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        candidates.push(local.join(r"Microsoft\WinGet\Links\ffmpeg.exe"));
    }
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        candidates.push(home.join(r"scoop\shims\ffmpeg.exe"));
    }
    if let Some(data) = std::env::var_os("ProgramData").map(PathBuf::from) {
        candidates.push(data.join(r"chocolatey\bin\ffmpeg.exe"));
    }
    candidates.push(r"C:\ffmpeg\bin\ffmpeg.exe".into());
    candidates.into_iter().find(|p| is_executable(p))
}

#[cfg(not(windows))]
fn find_ffmpeg() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("SATSUMA_FFMPEG") {
        candidates.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../Resources/bin/ffmpeg"));
            candidates.push(dir.join("ffmpeg"));
        }
    }
    candidates.push("/opt/homebrew/bin/ffmpeg".into());
    candidates.push("/usr/local/bin/ffmpeg".into());
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join("ffmpeg")));
    }
    candidates.into_iter().find(|p| is_executable(p))
}

fn list_encoders(ffmpeg: &Path) -> HashSet<String> {
    let Ok((_, stdout, _)) = crate::process::capture(Command::new(ffmpeg).args(["-hide_banner", "-encoders"])) else {
        return HashSet::new();
    };
    parse_encoders(&stdout)
}

/// Parses `ffmpeg -encoders` output (" A....D libmp3lame  libmp3lame MP3 …").
pub(crate) fn parse_encoders(text: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut past_header = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("------") {
            past_header = true;
            continue;
        }
        if !past_header {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let (Some(flags), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if flags.len() == 6 {
            set.insert(name.to_string());
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_encoder_list() {
        let text = "Encoders:\n V..... = Video\n ------\n V....D libx264  libx264 H.264\n A....D libmp3lame  MP3\n";
        let set = parse_encoders(text);
        assert!(set.contains("libx264"));
        assert!(set.contains("libmp3lame"));
        assert!(!set.contains("V....."));
    }
}
