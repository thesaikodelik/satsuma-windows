//! RTF → HTML for Windows (macOS uses `textutil`). Handles what word processors and
//! TextEdit write: paragraphs, line breaks, tabs, bold/italic/underline/strike, super- and
//! subscript, text colours, Unicode escapes, code pages, list bullets and field results.

use crate::html::escape;

#[derive(Clone, Copy, PartialEq, Default)]
struct Fmt {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    sup: bool,
    sub: bool,
    color: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum Dest {
    Text,
    Skip,
    ColorTable,
}

#[derive(Clone, Copy)]
struct State {
    fmt: Fmt,
    dest: Dest,
    /// Characters to skip after `\uN` (`\ucN`).
    uc: usize,
}

/// Destinations whose contents are never shown.
const SKIPPED: &[&str] = &[
    "fonttbl", "stylesheet", "info", "pict", "header", "headerl", "headerr", "headerf", "footer", "footerl",
    "footerr", "footerf", "footnote", "fldinst", "object", "themedata", "colorschememapping", "datastore",
    "latentstyles", "listtable", "listoverridetable", "rsidtbl", "generator", "xmlnstbl", "mmathPr",
    "pgdsctbl", "filetbl", "revtbl", "bkmkstart", "bkmkend", "nonshppict", "wgrffmtfilter",
    "operator", "author", "title", "company", "expandedcolortbl", "protusertbl", "xe", "tc", "annotation",
];

/// Windows-1252 bytes 0x80–0x9F (the rest of the range matches Latin-1).
pub fn cp1252_char(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D,
        0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161,
        0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match b {
        0x80..=0x9F => char::from_u32(HIGH[(b - 0x80) as usize] as u32).unwrap_or('?'),
        _ => b as char,
    }
}

struct Writer {
    colors: Vec<Option<(u8, u8, u8)>>,
    pending_color: (u8, u8, u8),
    paragraphs: Vec<String>,
    current: String,
    open: Option<Fmt>,
}

impl Writer {
    fn close_run(&mut self) {
        if let Some(f) = self.open.take() {
            if f.color > 0 && self.colors.get(f.color).copied().flatten().is_some() {
                self.current.push_str("</span>");
            }
            for (on, tag) in [(f.sub, "sub"), (f.sup, "sup"), (f.strike, "s"), (f.underline, "u"), (f.italic, "em"), (f.bold, "strong")] {
                if on {
                    self.current.push_str(&format!("</{tag}>"));
                }
            }
        }
    }

    fn text(&mut self, fmt: Fmt, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.open != Some(fmt) {
            self.close_run();
            for (on, tag) in [(fmt.bold, "strong"), (fmt.italic, "em"), (fmt.underline, "u"), (fmt.strike, "s"), (fmt.sup, "sup"), (fmt.sub, "sub")] {
                if on {
                    self.current.push_str(&format!("<{tag}>"));
                }
            }
            if let Some(Some((r, g, b))) = self.colors.get(fmt.color).copied().filter(|_| fmt.color > 0) {
                self.current.push_str(&format!("<span style=\"color: #{r:02x}{g:02x}{b:02x}\">"));
            }
            self.open = Some(fmt);
        }
        self.current.push_str(&escape(s));
    }

    fn raw(&mut self, html: &str) {
        self.close_run();
        self.current.push_str(html);
    }

    fn end_paragraph(&mut self) {
        self.close_run();
        let p = std::mem::take(&mut self.current);
        self.paragraphs.push(p);
    }
}

/// The body HTML (a `<p>` per paragraph) of an RTF document.
pub fn to_html_body(rtf: &[u8]) -> String {
    let mut w = Writer { colors: Vec::new(), pending_color: (0, 0, 0), paragraphs: Vec::new(), current: String::new(), open: None };
    let mut stack: Vec<State> = Vec::new();
    let mut st = State { fmt: Fmt::default(), dest: Dest::Text, uc: 1 };
    let mut skip_chars = 0usize;
    let mut group_start = false;
    let mut ignorable = false;
    let mut text = String::new();
    let mut i = 0;

    macro_rules! flush {
        () => {
            if !text.is_empty() {
                if st.dest == Dest::Text {
                    w.text(st.fmt, &text);
                }
                text.clear();
            }
        };
    }

    while i < rtf.len() {
        let c = rtf[i];
        match c {
            b'{' => {
                flush!();
                stack.push(st);
                group_start = true;
                ignorable = false;
                i += 1;
                continue;
            }
            b'}' => {
                flush!();
                if let Some(prev) = stack.pop() {
                    st = prev;
                }
                group_start = false;
                i += 1;
                continue;
            }
            b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            b'\\' => {
                i += 1;
                let Some(&n) = rtf.get(i) else { break };
                if n.is_ascii_alphabetic() {
                    let start = i;
                    while i < rtf.len() && rtf[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word = std::str::from_utf8(&rtf[start..i]).unwrap_or("");
                    let pstart = i;
                    if i < rtf.len() && (rtf[i] == b'-' || rtf[i].is_ascii_digit()) {
                        i += 1;
                        while i < rtf.len() && rtf[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let param: Option<i32> = std::str::from_utf8(&rtf[pstart..i]).ok().and_then(|s| s.parse().ok());
                    if i < rtf.len() && rtf[i] == b' ' {
                        i += 1;
                    }
                    let first = group_start;
                    group_start = false;
                    if skip_chars > 0 {
                        skip_chars -= 1;
                        continue;
                    }
                    if first && (ignorable || SKIPPED.contains(&word)) {
                        // `{\*\dest …}` and known non-text destinations.
                        if word == "colortbl" {
                            flush!();
                            st.dest = Dest::ColorTable;
                        } else if !(ignorable && word == "shppict") {
                            flush!();
                            st.dest = Dest::Skip;
                        }
                        ignorable = false;
                        continue;
                    }
                    if first && word == "colortbl" {
                        flush!();
                        st.dest = Dest::ColorTable;
                        continue;
                    }
                    let on = param.map_or(true, |p| p != 0);
                    match word {
                        "par" | "sect" => {
                            flush!();
                            if st.dest == Dest::Text {
                                w.end_paragraph();
                            }
                        }
                        "line" => {
                            flush!();
                            if st.dest == Dest::Text {
                                w.raw("<br />");
                            }
                        }
                        "tab" => text.push('\t'),
                        "emdash" => text.push('—'),
                        "endash" => text.push('–'),
                        "bullet" => text.push('•'),
                        "lquote" => text.push('‘'),
                        "rquote" => text.push('’'),
                        "ldblquote" => text.push('“'),
                        "rdblquote" => text.push('”'),
                        "emspace" | "enspace" | "qmspace" => text.push(' '),
                        "u" => {
                            if let Some(p) = param {
                                let code = if p < 0 { (p + 65536) as u32 } else { p as u32 };
                                // Surrogate pairs arrive as two \u escapes.
                                if (0xD800..0xDC00).contains(&code) {
                                    text.push_str(&format!("\u{0}{code}"));
                                } else if (0xDC00..0xE000).contains(&code) {
                                    if let Some(pos) = text.rfind('\u{0}') {
                                        let hi: u32 = text[pos + 1..].parse().unwrap_or(0);
                                        text.truncate(pos);
                                        if let Some(ch) = char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (code - 0xDC00)) {
                                            text.push(ch);
                                        }
                                    }
                                } else if let Some(ch) = char::from_u32(code) {
                                    text.push(ch);
                                }
                                skip_chars = st.uc;
                            }
                        }
                        "uc" => st.uc = param.unwrap_or(1).max(0) as usize,
                        "red" | "green" | "blue" if st.dest == Dest::ColorTable => {
                            let v = param.unwrap_or(0).clamp(0, 255) as u8;
                            match word {
                                "red" => w.pending_color.0 = v,
                                "green" => w.pending_color.1 = v,
                                _ => w.pending_color.2 = v,
                            }
                        }
                        "b" => {
                            flush!();
                            st.fmt.bold = on;
                        }
                        "i" => {
                            flush!();
                            st.fmt.italic = on;
                        }
                        "ul" | "uld" | "uldb" | "ulw" => {
                            flush!();
                            st.fmt.underline = on;
                        }
                        "ulnone" => {
                            flush!();
                            st.fmt.underline = false;
                        }
                        "strike" | "striked" => {
                            flush!();
                            st.fmt.strike = on;
                        }
                        "super" => {
                            flush!();
                            st.fmt.sup = on;
                        }
                        "sub" => {
                            flush!();
                            st.fmt.sub = on;
                        }
                        "nosupersub" => {
                            flush!();
                            st.fmt.sup = false;
                            st.fmt.sub = false;
                        }
                        "cf" => {
                            flush!();
                            st.fmt.color = param.unwrap_or(0).max(0) as usize;
                        }
                        "plain" => {
                            flush!();
                            st.fmt = Fmt::default();
                        }
                        _ => {}
                    }
                } else {
                    i += 1;
                    let first = group_start;
                    group_start = false;
                    match n {
                        b'*' if first => {
                            ignorable = true;
                            group_start = true;
                        }
                        b'\'' => {
                            let hex = rtf.get(i..i + 2).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                            i += 2;
                            if skip_chars > 0 {
                                skip_chars -= 1;
                            } else if let Some(b) = hex {
                                text.push(cp1252_char(b));
                            }
                        }
                        b'\\' | b'{' | b'}' => text.push(n as char),
                        b'~' => text.push('\u{a0}'),
                        b'_' => text.push('\u{2011}'),
                        b'\r' | b'\n' => {
                            flush!();
                            if st.dest == Dest::Text {
                                w.end_paragraph();
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }
            _ => {}
        }
        group_start = false;
        if skip_chars > 0 {
            skip_chars -= 1;
            i += 1;
            continue;
        }
        match st.dest {
            Dest::ColorTable => {
                if c == b';' {
                    // The first, empty entry is "auto".
                    let entry = if w.colors.is_empty() && w.pending_color == (0, 0, 0) { None } else { Some(w.pending_color) };
                    w.colors.push(entry);
                    w.pending_color = (0, 0, 0);
                }
            }
            Dest::Skip => {}
            Dest::Text => {
                // Bytes outside escapes are ASCII in valid RTF; decode leniently.
                if c < 0x80 {
                    text.push(c as char);
                } else {
                    text.push(cp1252_char(c));
                }
            }
        }
        i += 1;
    }
    flush!();
    if !w.current.is_empty() {
        w.end_paragraph();
    }

    // Drop trailing empty paragraphs; keep inner blank lines as spacing.
    while w.paragraphs.last().is_some_and(|p| p.trim().is_empty()) {
        w.paragraphs.pop();
    }
    let mut body = String::new();
    for p in &w.paragraphs {
        let html = p.replace('\t', "&emsp;");
        if html.trim().is_empty() {
            body.push_str("<p><br /></p>\n");
        } else {
            body.push_str(&format!("<p>{html}</p>\n"));
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_formatting_colours_and_unicode() {
        let rtf = br"{\rtf1\ansi\ansicpg1252{\fonttbl\f0\fswiss Helvetica;}{\colortbl;\red255\green0\blue0;}
\f0\fs24 Hello {\b bold} and {\i italic}\par
\cf1 red\cf0  caf\'e9 \u8364? end\line next\par
{\*\generator Riched20;}{\field{\*\fldinst HYPERLINK x}{\fldrslt link}}\par}";
        let html = to_html_body(rtf);
        assert!(html.contains("Hello <strong>bold</strong> and <em>italic</em>"), "{html}");
        assert!(html.contains("<span style=\"color: #ff0000\">red</span>"), "{html}");
        assert!(html.contains("café €"), "{html}");
        assert!(html.contains("end<br />next"), "{html}");
        assert!(html.contains("<p>link</p>"), "{html}");
        assert!(!html.contains("Helvetica") && !html.contains("Riched") && !html.contains("HYPERLINK"), "{html}");
    }

    #[test]
    fn surrogate_pairs_and_escapes() {
        let html = to_html_body(br"{\rtf1 a\{b\}\\c \u-10179?\u-8704? z}");
        assert!(html.contains("a{b}\\c \u{1F600} z"), "{html}");
    }
}
