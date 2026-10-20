//! What `textutil -convert <to> -output out in` does on macOS, for the conversions Satsuma
//! uses: docx / rtf / rtfd / txt / html → html, and html → txt / docx.

use crate::html::{attr, decode_entities, document, escape, tokenize, Token};
use crate::{Error, Result};
use base64::Engine;
use satsuma_core::naming::stem_of;
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;

fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    // UTF-16 text files (Notepad's "Unicode") are common on Windows.
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.to_string()),
        // Legacy ANSI text: decode as Windows-1252.
        Err(_) => Ok(bytes.iter().map(|&b| if b < 0x80 { b as char } else { crate::win::rtf::cp1252_char(b) }).collect()),
    }
}

/// Any supported input as an HTML document.
pub fn load_html(input: &Path) -> Result<String> {
    let title = stem_of(&input.file_name().unwrap_or_default().to_string_lossy()).to_string();
    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "docx" => super::docx_read::to_html(input, &title),
        "rtf" => Ok(document(&title, &super::rtf::to_html_body(&std::fs::read(input)?))),
        "rtfd" => {
            let rtf = ["TXT.rtf", "txt.rtf"].iter().map(|n| input.join(n)).find(|p| p.is_file());
            let rtf = rtf.ok_or_else(|| Error::msg("This RTFD document has no text."))?;
            Ok(document(&title, &super::rtf::to_html_body(&std::fs::read(rtf)?)))
        }
        "html" | "htm" | "xhtml" => read_text(input),
        _ => Ok(document(&title, &crate::doc::text_to_html_body(&read_text(input)?))),
    }
}

pub fn convert(input: &Path, out: &Path, to: &str) -> Result<()> {
    let html = load_html(input)?;
    match to {
        "html" => std::fs::write(out, html)?,
        "txt" => std::fs::write(out, html_to_text(&html))?,
        "docx" => html_to_docx(&html, out, input.parent())?,
        other => return Err(Error::msg(format!("Cannot convert documents to {other} here."))),
    }
    Ok(())
}

const BLOCKS: &[&str] = &[
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "ul", "ol", "tr", "table", "pre", "blockquote", "section",
    "article", "header", "footer", "dl", "dt", "dd", "figure", "figcaption", "hr",
];
const HIDDEN: &[&str] = &["head", "script", "style", "title", "template"];

/// Plain text, one block per line (what `textutil -convert txt` writes).
pub fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut hidden = 0usize;
    let mut pre = 0usize;
    let mut lists: Vec<Option<usize>> = Vec::new();
    let mut first_cell = true;
    let newline = |out: &mut String| {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    };
    for t in tokenize(crate::html::body_of(html)) {
        match t {
            Token::Open { name, attrs, self_closing } => {
                let n = name.as_str();
                if HIDDEN.contains(&n) && !self_closing {
                    hidden += 1;
                    continue;
                }
                match n {
                    "br" => out.push('\n'),
                    "pre" => {
                        newline(&mut out);
                        pre += 1;
                    }
                    "ul" => lists.push(None),
                    "ol" => lists.push(Some(attr(attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1))),
                    "li" => {
                        newline(&mut out);
                        out.push_str(&"\t".repeat(lists.len().saturating_sub(1)));
                        match lists.last_mut() {
                            Some(Some(k)) => {
                                out.push_str(&format!("{k}.\t"));
                                *k += 1;
                            }
                            _ => out.push_str("•\t"),
                        }
                    }
                    "tr" => {
                        newline(&mut out);
                        first_cell = true;
                    }
                    "td" | "th" => {
                        if !first_cell {
                            out.push('\t');
                        }
                        first_cell = false;
                    }
                    "img" => {
                        if let Some(alt) = attr(attrs, "alt").filter(|a| !a.is_empty()) {
                            out.push_str(&alt);
                        }
                    }
                    _ if BLOCKS.contains(&n) => newline(&mut out),
                    _ => {}
                }
            }
            Token::Close { name } => {
                let n = name.as_str();
                if HIDDEN.contains(&n) {
                    hidden = hidden.saturating_sub(1);
                    continue;
                }
                match n {
                    "pre" => {
                        pre = pre.saturating_sub(1);
                        newline(&mut out);
                    }
                    "ul" | "ol" => {
                        lists.pop();
                        newline(&mut out);
                    }
                    _ if BLOCKS.contains(&n) => newline(&mut out),
                    _ => {}
                }
            }
            Token::Text(s) => {
                if hidden > 0 {
                    continue;
                }
                let text = decode_entities(s);
                if pre > 0 {
                    out.push_str(&text);
                } else {
                    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    if collapsed.is_empty() {
                        if !out.is_empty() && !out.ends_with(['\n', ' ', '\t']) && text.chars().any(char::is_whitespace) {
                            out.push(' ');
                        }
                        continue;
                    }
                    if text.starts_with(char::is_whitespace) && !out.is_empty() && !out.ends_with(['\n', ' ', '\t']) {
                        out.push(' ');
                    }
                    out.push_str(&collapsed);
                    if text.ends_with(char::is_whitespace) {
                        out.push(' ');
                    }
                }
            }
        }
    }
    let lines: Vec<&str> = out.lines().map(|l| l.trim_end()).collect();
    let mut text = lines.join("\n").trim().to_string();
    text.push('\n');
    text
}

// ---------------------------------------------------------------------------------------
// HTML → DOCX

#[derive(Clone, Copy, Default, PartialEq)]
struct Inline {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    code: bool,
    sup: bool,
    sub: bool,
}

struct Picture {
    data: Vec<u8>,
    ext: &'static str,
    w: u32,
    h: u32,
}

struct DocxWriter {
    body: String,
    para: Option<String>,
    para_props: String,
    pictures: Vec<Picture>,
    links: Vec<String>,
    /// The open link, if any. A hyperlink can't cross paragraphs in Word, so it is closed
    /// at the end of each paragraph and reopened in the next.
    link: Option<usize>,
}

impl DocxWriter {
    fn ensure_para(&mut self) -> &mut String {
        if self.para.is_none() {
            self.para = Some(match self.link {
                Some(id) => format!("<w:hyperlink r:id=\"rIdLink{id}\">"),
                None => String::new(),
            });
        }
        self.para.as_mut().unwrap()
    }

    fn open_link(&mut self, href: String) {
        self.close_link();
        self.links.push(href);
        let id = self.links.len();
        self.ensure_para().push_str(&format!("<w:hyperlink r:id=\"rIdLink{id}\">"));
        self.link = Some(id);
    }

    fn close_link(&mut self) {
        if self.link.take().is_some() {
            if let Some(para) = self.para.as_mut() {
                para.push_str("</w:hyperlink>");
            }
        }
    }

    fn end_para(&mut self) {
        if self.link.is_some() {
            if let Some(para) = self.para.as_mut() {
                para.push_str("</w:hyperlink>");
            }
        }
        if let Some(runs) = self.para.take() {
            self.body.push_str(&format!("<w:p>{}{runs}</w:p>", if self.para_props.is_empty() { String::new() } else { format!("<w:pPr>{}</w:pPr>", self.para_props) }));
        }
    }

    fn run(&mut self, fmt: Inline, text: &str) {
        let mut rpr = String::new();
        if fmt.bold {
            rpr.push_str("<w:b/>");
        }
        if fmt.italic {
            rpr.push_str("<w:i/>");
        }
        if fmt.underline {
            rpr.push_str("<w:u w:val=\"single\"/>");
        }
        if fmt.strike {
            rpr.push_str("<w:strike/>");
        }
        if fmt.code {
            rpr.push_str("<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/>");
        }
        if fmt.sup {
            rpr.push_str("<w:vertAlign w:val=\"superscript\"/>");
        } else if fmt.sub {
            rpr.push_str("<w:vertAlign w:val=\"subscript\"/>");
        }
        let rpr = if rpr.is_empty() { String::new() } else { format!("<w:rPr>{rpr}</w:rPr>") };
        let para = self.ensure_para();
        for (i, part) in text.split('\t').enumerate() {
            if i > 0 {
                para.push_str(&format!("<w:r>{rpr}<w:tab/></w:r>"));
            }
            if !part.is_empty() {
                para.push_str(&format!("<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>", escape(part)));
            }
        }
    }

    fn picture(&mut self, pic: Picture) {
        let n = self.pictures.len() + 1;
        // Fit the 6.5" × 9" text area, never enlarging beyond 96 dpi.
        let (w_in, h_in) = (pic.w as f64 / 96.0, pic.h as f64 / 96.0);
        let s = (6.5 / w_in).min(9.0 / h_in).min(1.0);
        let (cx, cy) = ((w_in * s * 914_400.0) as u64, (h_in * s * 914_400.0) as u64);
        let xml = format!(
            r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{n}" name="Picture {n}"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic><pic:nvPicPr><pic:cNvPr id="{n}" name="image{n}.{ext}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage{n}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
            ext = pic.ext
        );
        self.ensure_para().push_str(&xml);
        self.pictures.push(pic);
    }
}

fn load_picture(src: &str, base: Option<&Path>) -> Option<Picture> {
    let bytes = if let Some(data) = src.strip_prefix("data:") {
        let (_, b64) = data.split_once(";base64,")?;
        base64::engine::general_purpose::STANDARD.decode(b64.trim()).ok()?
    } else if src.contains("://") {
        return None;
    } else {
        let path = base?.join(src.replace('/', "\\"));
        std::fs::read(path).ok()?
    };
    let img = image::load_from_memory(&bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    match image::guess_format(&bytes).ok()? {
        image::ImageFormat::Png => Some(Picture { data: bytes, ext: "png", w, h }),
        image::ImageFormat::Jpeg => Some(Picture { data: bytes, ext: "jpeg", w, h }),
        image::ImageFormat::Gif => Some(Picture { data: bytes, ext: "gif", w, h }),
        _ => {
            let mut png = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some(Picture { data: png, ext: "png", w, h })
        }
    }
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>{HEADINGS}<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="720" w:right="720"/></w:pPr><w:rPr><w:i/><w:color w:val="404040"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/><w:sz w:val="19"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="40"/></w:pPr></w:style><w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:color="BFBFBF"/><w:left w:val="single" w:sz="4" w:color="BFBFBF"/><w:bottom w:val="single" w:sz="4" w:color="BFBFBF"/><w:right w:val="single" w:sz="4" w:color="BFBFBF"/><w:insideH w:val="single" w:sz="4" w:color="BFBFBF"/><w:insideV w:val="single" w:sz="4" w:color="BFBFBF"/></w:tblBorders></w:tblPr></w:style><w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style></w:styles>"#;

fn heading_styles() -> String {
    let sizes = [32, 26, 24, 22, 22, 22];
    (1..=6)
        .map(|l| {
            format!(
                r#"<w:style w:type="paragraph" w:styleId="Heading{l}"><w:name w:val="heading {l}"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="80"/><w:outlineLvl w:val="{}"/></w:pPr><w:rPr><w:b/><w:color w:val="1F3864"/><w:sz w:val="{}"/></w:rPr></w:style>"#,
                l - 1,
                sizes[l - 1]
            )
        })
        .collect()
}

/// Writes HTML as a Word document with real headings, lists, tables, links and pictures.
pub fn html_to_docx(html: &str, out: &Path, base: Option<&Path>) -> Result<()> {
    let mut w = DocxWriter { body: String::new(), para: None, para_props: String::new(), pictures: Vec::new(), links: Vec::new(), link: None };
    let mut fmt_stack: Vec<(String, Inline)> = Vec::new();
    let mut fmt = Inline::default();
    let mut hidden = 0usize;
    let mut pre = 0usize;
    let mut quote = 0usize;
    let mut lists: Vec<Option<usize>> = Vec::new();
    let mut in_table = 0usize;
    let mut link_open = false;

    for t in tokenize(crate::html::body_of(html)) {
        match t {
            Token::Open { name, attrs, self_closing } => {
                let n = name.as_str();
                if HIDDEN.contains(&n) && !self_closing {
                    hidden += 1;
                    continue;
                }
                let mut push_fmt = |f: &mut Inline, change: fn(&mut Inline)| {
                    fmt_stack.push((name.clone(), *f));
                    change(f);
                };
                match n {
                    "strong" | "b" => push_fmt(&mut fmt, |f| f.bold = true),
                    "em" | "i" | "cite" => push_fmt(&mut fmt, |f| f.italic = true),
                    "u" | "ins" => push_fmt(&mut fmt, |f| f.underline = true),
                    "s" | "del" | "strike" => push_fmt(&mut fmt, |f| f.strike = true),
                    "code" | "kbd" | "samp" | "tt" => push_fmt(&mut fmt, |f| f.code = true),
                    "sup" => push_fmt(&mut fmt, |f| f.sup = true),
                    "sub" => push_fmt(&mut fmt, |f| f.sub = true),
                    "a" => {
                        if let Some(href) = attr(attrs, "href").filter(|h| h.contains("://") || h.starts_with("mailto:")) {
                            w.open_link(href);
                            link_open = true;
                            push_fmt(&mut fmt, |f| f.underline = true);
                        }
                    }
                    "br" => w.ensure_para().push_str("<w:r><w:br/></w:r>"),
                    "img" => {
                        if let Some(p) = attr(attrs, "src").and_then(|s| load_picture(&s, base)) {
                            w.picture(p);
                        }
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        w.end_para();
                        w.para_props = format!("<w:pStyle w:val=\"Heading{}\"/>", &n[1..]);
                    }
                    "p" | "div" | "dt" | "dd" | "figcaption" | "section" | "article" => {
                        if in_table == 0 {
                            w.end_para();
                            w.para_props = if quote > 0 { "<w:pStyle w:val=\"Quote\"/>".into() } else { String::new() };
                        }
                    }
                    "blockquote" => {
                        w.end_para();
                        quote += 1;
                        w.para_props = "<w:pStyle w:val=\"Quote\"/>".into();
                    }
                    "pre" => {
                        w.end_para();
                        pre += 1;
                        w.para_props = "<w:pStyle w:val=\"Code\"/>".into();
                    }
                    "ul" => {
                        w.end_para();
                        lists.push(None);
                    }
                    "ol" => {
                        w.end_para();
                        lists.push(Some(attr(attrs, "start").and_then(|s| s.parse().ok()).unwrap_or(1)));
                    }
                    "li" => {
                        w.end_para();
                        let depth = lists.len().max(1);
                        w.para_props = format!("<w:pStyle w:val=\"ListParagraph\"/><w:ind w:left=\"{}\" w:hanging=\"360\"/>", 360 * depth + 360);
                        let marker = match lists.last_mut() {
                            Some(Some(k)) => {
                                let m = format!("{k}.\t");
                                *k += 1;
                                m
                            }
                            _ => "•\t".to_string(),
                        };
                        w.run(Inline::default(), &marker);
                    }
                    "hr" => {
                        w.end_para();
                        w.body.push_str(r#"<w:p><w:pPr><w:pBdr><w:bottom w:val="single" w:sz="6" w:space="1" w:color="BFBFBF"/></w:pBdr></w:pPr></w:p>"#);
                    }
                    "table" => {
                        w.end_para();
                        in_table += 1;
                        w.body.push_str(r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/></w:tblPr>"#);
                    }
                    "tr" => w.body.push_str("<w:tr>"),
                    "td" | "th" => {
                        w.body.push_str("<w:tc><w:tcPr><w:tcW w:w=\"0\" w:type=\"auto\"/></w:tcPr>");
                        w.para_props = String::new();
                        if n == "th" {
                            push_fmt(&mut fmt, |f| f.bold = true);
                        }
                    }
                    _ => {}
                }
            }
            Token::Close { name } => {
                let n = name.as_str();
                if HIDDEN.contains(&n) {
                    hidden = hidden.saturating_sub(1);
                    continue;
                }
                if let Some(pos) = fmt_stack.iter().rposition(|(tag, _)| tag == n) {
                    fmt = fmt_stack[pos].1;
                    fmt_stack.truncate(pos);
                }
                match n {
                    "a" if link_open => {
                        w.close_link();
                        link_open = false;
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "dt" | "dd" | "figcaption" => {
                        w.end_para();
                        w.para_props = String::new();
                    }
                    "p" | "div" | "section" | "article" if in_table == 0 => {
                        w.end_para();
                        w.para_props = if quote > 0 { "<w:pStyle w:val=\"Quote\"/>".into() } else { String::new() };
                    }
                    "blockquote" => {
                        w.end_para();
                        quote = quote.saturating_sub(1);
                        w.para_props = String::new();
                    }
                    "pre" => {
                        w.end_para();
                        pre = pre.saturating_sub(1);
                        w.para_props = String::new();
                    }
                    "ul" | "ol" => {
                        w.end_para();
                        lists.pop();
                    }
                    "td" | "th" => {
                        // Every cell needs at least one paragraph.
                        if w.para.is_none() {
                            w.para = Some(String::new());
                        }
                        w.end_para();
                        w.body.push_str("</w:tc>");
                    }
                    "tr" => w.body.push_str("</w:tr>"),
                    "table" => {
                        in_table = in_table.saturating_sub(1);
                        w.body.push_str("</w:tbl><w:p/>");
                    }
                    _ => {}
                }
            }
            Token::Text(s) => {
                if hidden > 0 {
                    continue;
                }
                let text = decode_entities(s);
                if pre > 0 {
                    for (i, line) in text.split('\n').enumerate() {
                        if i > 0 {
                            w.end_para();
                            w.para_props = "<w:pStyle w:val=\"Code\"/>".into();
                        }
                        let line = line.trim_end_matches('\r');
                        w.run(Inline { code: false, ..fmt }, line);
                    }
                    continue;
                }
                let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if collapsed.is_empty() {
                    if w.para.as_ref().is_some_and(|p| !p.is_empty()) && text.chars().any(char::is_whitespace) {
                        w.run(fmt, " ");
                    }
                    continue;
                }
                let lead = if text.starts_with(char::is_whitespace) && w.para.as_ref().is_some_and(|p| !p.is_empty()) { " " } else { "" };
                let trail = if text.ends_with(char::is_whitespace) { " " } else { "" };
                w.run(fmt, &format!("{lead}{collapsed}{trail}"));
            }
        }
    }
    w.end_para();
    if w.body.is_empty() {
        w.body.push_str("<w:p/>");
    }
    write_package(&w, out)
}

fn write_package(w: &DocxWriter, out: &Path) -> Result<()> {
    let z = |e: zip::result::ZipError| Error::msg(format!("Could not write the Word document: {e}"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(out)?);
    let opts = SimpleFileOptions::default();

    zip.start_file("[Content_Types].xml", opts).map_err(z)?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Default Extension="gif" ContentType="image/gif"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#)?;
    zip.start_file("_rels/.rels", opts).map_err(z)?;
    zip.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#)?;

    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#);
    for (i, p) in w.pictures.iter().enumerate() {
        rels.push_str(&format!(
            r#"<Relationship Id="rIdImage{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{n}.{ext}"/>"#,
            n = i + 1,
            ext = p.ext
        ));
    }
    for (i, href) in w.links.iter().enumerate() {
        rels.push_str(&format!(
            r#"<Relationship Id="rIdLink{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
            i + 1,
            escape(href)
        ));
    }
    rels.push_str("</Relationships>");
    zip.start_file("word/_rels/document.xml.rels", opts).map_err(z)?;
    zip.write_all(rels.as_bytes())?;
    zip.start_file("word/styles.xml", opts).map_err(z)?;
    zip.write_all(STYLES.replace("{HEADINGS}", &heading_styles()).as_bytes())?;
    for (i, p) in w.pictures.iter().enumerate() {
        zip.start_file(format!("word/media/image{}.{}", i + 1, p.ext), SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored))
            .map_err(z)?;
        zip.write_all(&p.data)?;
    }
    zip.start_file("word/document.xml", opts).map_err(z)?;
    zip.write_all(
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>{}<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
            w.body
        )
        .as_bytes(),
    )?;
    zip.finish().map_err(z)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_blocks_lists_and_cells() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body><h1>Title</h1><p>Hello <b>big</b>\n world</p><ul><li>one</li><li>two</li></ul><ol><li>a</li></ol><table><tr><td>x</td><td>y</td></tr></table></body></html>";
        assert_eq!(html_to_text(html), "Title\nHello big world\n•\tone\n•\ttwo\n1.\ta\nx\ty\n");
    }

    #[test]
    fn docx_round_trips_structure() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("d.docx");
        let html = "<h2>Head</h2><p>Some <strong>bold</strong> &amp; <em>it</em> <a href=\"https://x.org\">link</a></p><ul><li>item</li></ul><table><tr><th>A</th><td>B</td></tr></table><pre>l1\nl2</pre>";
        html_to_docx(html, &out, None).unwrap();
        let back = super::super::docx_read::to_html(&out, "d").unwrap();
        assert!(back.contains("<h2>Head</h2>"), "{back}");
        assert!(back.contains("<strong>bold</strong>"), "{back}");
        assert!(back.contains("&amp;"), "{back}");
        assert!(back.contains("<a href=\"https://x.org\"><u>link</u></a>"), "{back}");
        // A link wrapping two paragraphs stays well-formed.
        let out2 = dir.path().join("b.docx");
        html_to_docx("<a href=\"https://y.org\"><p>one</p><p>two</p></a><p>after</p>", &out2, None).unwrap();
        let back2 = super::super::docx_read::to_html(&out2, "b").unwrap();
        assert!(back2.contains("one") && back2.contains("two") && back2.contains("after"), "{back2}");
        assert!(back.contains("<td><strong>A</strong></td><td>B</td>"), "{back}");
        assert!(back.contains("l1") && back.contains("l2"), "{back}");
        assert!(back.contains("item"), "{back}");
    }
}
