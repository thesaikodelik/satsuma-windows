//! HTML → PDF with headless Microsoft Edge (Chromium's print pipeline), which ships with
//! Windows 10 and 11. This takes the place of `textutil` + CoreText pagination on macOS.

use crate::env::Env;
use crate::process::{run, Cancel};
use crate::{Error, Result};
use std::path::Path;
use std::process::Command;

/// US Letter with 1" margins and system fonts, like the macOS output.
const PRINT_CSS: &str = "<style>\
@page { size: 8.5in 11in; margin: 1in; }\
html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }\
body { font-family: 'Segoe UI', 'Helvetica Neue', Arial, sans-serif; font-size: 11pt; line-height: 1.4; margin: 0; }\
img, svg { max-width: 100%; height: auto; }\
pre, code { font-family: Consolas, 'Courier New', monospace; font-size: 9.5pt; }\
pre { white-space: pre-wrap; word-wrap: break-word; }\
table { border-collapse: collapse; }\
td, th { border: 1px solid #c8c8c8; padding: 3px 6px; vertical-align: top; }\
</style>";

fn with_print_css(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if let Some(i) = lower.find("</head>") {
        format!("{}{PRINT_CSS}{}", &html[..i], &html[i..])
    } else if let Some(i) = lower.find("<body") {
        format!("{}<head>{PRINT_CSS}</head>{}", &html[..i], &html[i..])
    } else {
        format!("<!DOCTYPE html><html><head><meta charset=\"utf-8\">{PRINT_CSS}</head><body>{html}</body></html>")
    }
}

/// `file:///C:/dir/page%20one.html`
pub fn file_url(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let s = abs.to_string_lossy().replace('\\', "/");
    let s = s.trim_start_matches("//?/");
    let mut url = String::from("file:///");
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => url.push(b as char),
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

/// Prints `html` (a complete document) to `out` as a paginated PDF.
pub fn print_to_pdf(env: &Env, html: &str, out: &Path, tmp: &Path, cancel: &Cancel) -> Result<()> {
    let browser = env
        .browser
        .as_ref()
        .ok_or_else(|| Error::msg("Making PDFs from documents needs Microsoft Edge, which is not installed."))?;
    std::fs::create_dir_all(tmp)?;
    let page = crate::image::unique_in(tmp, "print.html");
    std::fs::write(&page, with_print_css(html))?;
    let profile = tmp.join("edge-profile");
    let pdf = crate::image::unique_in(tmp, "print.pdf");
    let mut cmd = Command::new(browser);
    cmd.args([
        "--headless",
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-features=msEdgeSidebarV2,msHubApps,msWebAssist",
        "--no-pdf-header-footer",
        "--run-all-compositor-stages-before-draw",
        "--allow-file-access-from-files",
    ]);
    cmd.arg(format!("--user-data-dir={}", profile.display()));
    cmd.arg(format!("--print-to-pdf={}", pdf.display()));
    cmd.arg(file_url(&page));
    run(&mut cmd, cancel, &mut |_| {})?;
    if !pdf.is_file() || std::fs::metadata(&pdf)?.len() == 0 {
        return Err(Error::msg("Edge did not produce a PDF."));
    }
    std::fs::rename(&pdf, out).or_else(|_| std::fs::copy(&pdf, out).map(|_| ()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_escape_spaces_and_unicode() {
        let url = file_url(Path::new(r"C:\Users\a b\é.html"));
        assert_eq!(url, "file:///C:/Users/a%20b/%C3%A9.html");
    }

    #[test]
    fn print_css_goes_into_the_head() {
        let html = with_print_css("<html><head><title>x</title></head><body>y</body></html>");
        assert!(html.contains("<title>x</title><style>@page"));
        assert!(with_print_css("hello").contains("<body>hello</body>"));
    }
}
