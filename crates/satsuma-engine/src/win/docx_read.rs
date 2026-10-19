//! Word (.docx) → HTML for Windows (macOS uses `textutil`): headings, paragraphs, runs
//! (bold, italic, underline, strike, super/subscript), line breaks and tabs, hyperlinks,
//! bulleted and numbered lists, tables and inline pictures (as data URIs).

use crate::html::{attr, decode_entities, escape, tokenize, Token};
use crate::{Error, Result};
use base64::Engine;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

fn part(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<Vec<u8>> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn text_part(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    part(zip, name).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Relationship id → target, from a `.rels` part.
fn relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for t in tokenize(xml) {
        if let Token::Open { name, attrs, .. } = t {
            if name == "relationship" {
                if let (Some(id), Some(target)) = (attr(attrs, "id"), attr(attrs, "target")) {
                    map.insert(id, target);
                }
            }
        }
    }
    map
}

/// Style id → heading level (1–6) for "heading N", "Title" and "Subtitle" styles.
fn heading_styles(xml: &str) -> HashMap<String, u8> {
    let mut map = HashMap::new();
    let mut current: Option<String> = None;
    for t in tokenize(xml) {
        match t {
            Token::Open { name, attrs, .. } if name == "w:style" => current = attr(attrs, "w:styleid"),
            Token::Open { name, attrs, .. } if name == "w:name" => {
                if let (Some(id), Some(n)) = (current.as_ref(), attr(attrs, "w:val")) {
                    let n = n.to_ascii_lowercase();
                    let level = if n == "title" {
                        Some(1)
                    } else if n == "subtitle" {
                        Some(2)
                    } else {
                        n.strip_prefix("heading ").and_then(|d| d.trim().parse::<u8>().ok()).map(|l| l.clamp(1, 6))
                    };
                    if let Some(l) = level {
                        map.insert(id.clone(), l);
                    }
                }
            }
            _ => {}
        }
    }
    map
}

/// (numId, level) → ordered list?, from `numbering.xml`.
fn numbering(xml: &str) -> HashMap<(String, u8), bool> {
    let mut abstract_fmt: HashMap<(String, u8), bool> = HashMap::new();
    let mut num_to_abstract: HashMap<String, String> = HashMap::new();
    let (mut abs, mut lvl, mut num): (Option<String>, u8, Option<String>) = (None, 0, None);
    for t in tokenize(xml) {
        match t {
            Token::Open { name, attrs, .. } => match name.as_str() {
                "w:abstractnum" => abs = attr(attrs, "w:abstractnumid"),
                "w:lvl" => lvl = attr(attrs, "w:ilvl").and_then(|v| v.parse().ok()).unwrap_or(0),
                "w:numfmt" => {
                    if let Some(a) = &abs {
                        let fmt = attr(attrs, "w:val").unwrap_or_default();
                        abstract_fmt.insert((a.clone(), lvl), fmt != "bullet" && fmt != "none");
                    }
                }
                "w:num" => num = attr(attrs, "w:numid"),
                "w:abstractnumid" => {
                    if let (Some(n), Some(a)) = (&num, attr(attrs, "w:val")) {
                        num_to_abstract.insert(n.clone(), a);
                    }
                }
                _ => {}
            },
            Token::Close { name } if name == "w:abstractnum" => abs = None,
            _ => {}
        }
    }
    let mut out = HashMap::new();
    for (n, a) in num_to_abstract {
        for l in 0..9u8 {
            if let Some(ordered) = abstract_fmt.get(&(a.clone(), l)) {
                out.insert((n.clone(), l), *ordered);
            }
        }
    }
    out
}

fn is_on(attrs: &str) -> bool {
    !matches!(attr(attrs, "w:val").as_deref(), Some("0" | "false" | "off" | "none"))
}

#[derive(Default, Clone, Copy, PartialEq)]
struct RunFmt {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    sup: bool,
    sub: bool,
}

impl RunFmt {
    fn wrap(self, text: &str) -> String {
        let mut s = text.to_string();
        for (on, tag) in [(self.sub, "sub"), (self.sup, "sup"), (self.strike, "s"), (self.underline, "u"), (self.italic, "em"), (self.bold, "strong")] {
            if on {
                s = format!("<{tag}>{s}</{tag}>");
            }
        }
        s
    }
}

#[derive(Default)]
struct Para {
    heading: Option<u8>,
    list: Option<(String, u8)>,
    html: String,
}

/// Converts a .docx file to an HTML document.
pub fn to_html(path: &Path, title: &str) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| Error::msg("This is not a Word document."))?;
    let doc = text_part(&mut zip, "word/document.xml").ok_or_else(|| Error::msg("This Word document has no text."))?;
    let rels = text_part(&mut zip, "word/_rels/document.xml.rels").map(|x| relationships(&x)).unwrap_or_default();
    let styles = text_part(&mut zip, "word/styles.xml").map(|x| heading_styles(&x)).unwrap_or_default();
    let lists = text_part(&mut zip, "word/numbering.xml").map(|x| numbering(&x)).unwrap_or_default();

    let mut body = String::new();
    // Open list stack: (ordered, level).
    let mut list_stack: Vec<(bool, u8)> = Vec::new();
    let mut para: Option<Para> = None;
    let mut run = RunFmt::default();
    let mut in_rpr = false;
    let mut in_text = false;
    let mut link: Option<String> = None;
    let mut table_depth = 0usize;
    let mut cell_paras = 0usize;
    let mut pending_num: (Option<String>, u8) = (None, 0);
    let mut in_ppr = false;

    let close_lists = |body: &mut String, stack: &mut Vec<(bool, u8)>, keep: usize| {
        while stack.len() > keep {
            let (ordered, _) = stack.pop().unwrap();
            body.push_str(if ordered { "</li></ol>\n" } else { "</li></ul>\n" });
        }
    };

    for t in tokenize(&doc) {
        match t {
            Token::Open { name, attrs, self_closing } => match name.as_str() {
                "w:p" => {
                    para = Some(Para::default());
                    pending_num = (None, 0);
                    if self_closing {
                        if table_depth == 0 {
                            close_lists(&mut body, &mut list_stack, 0);
                            body.push_str("<p><br /></p>\n");
                        }
                        para = None;
                    }
                }
                "w:ppr" => in_ppr = !self_closing,
                "w:pstyle" => {
                    if let (Some(p), Some(id)) = (para.as_mut(), attr(attrs, "w:val")) {
                        p.heading = styles.get(&id).copied().or_else(|| {
                            let l = id.to_ascii_lowercase();
                            l.strip_prefix("heading").and_then(|d| d.parse::<u8>().ok()).map(|d| d.clamp(1, 6))
                        });
                        if id.eq_ignore_ascii_case("Title") {
                            p.heading = Some(1);
                        }
                    }
                }
                "w:ilvl" if in_ppr => pending_num.1 = attr(attrs, "w:val").and_then(|v| v.parse().ok()).unwrap_or(0),
                "w:numid" if in_ppr => {
                    pending_num.0 = attr(attrs, "w:val").filter(|v| v != "0");
                    if let (Some(p), Some(id)) = (para.as_mut(), pending_num.0.clone()) {
                        p.list = Some((id, pending_num.1));
                    }
                }
                "w:r" => run = RunFmt::default(),
                "w:rpr" => in_rpr = !self_closing,
                "w:b" if in_rpr => run.bold = is_on(attrs),
                "w:i" if in_rpr => run.italic = is_on(attrs),
                "w:u" if in_rpr => run.underline = is_on(attrs),
                "w:strike" | "w:dstrike" if in_rpr => run.strike = is_on(attrs),
                "w:vertalign" if in_rpr => {
                    let v = attr(attrs, "w:val").unwrap_or_default();
                    run.sup = v == "superscript";
                    run.sub = v == "subscript";
                }
                "w:t" => in_text = !self_closing,
                "w:tab" if !in_ppr => {
                    if let Some(p) = para.as_mut() {
                        p.html.push_str("&emsp;");
                    }
                }
                "w:br" | "w:cr" => {
                    let page = attr(attrs, "w:type").is_some_and(|t| t == "page");
                    if let (Some(p), false) = (para.as_mut(), page) {
                        p.html.push_str("<br />");
                    }
                }
                "w:hyperlink" => {
                    link = attr(attrs, "r:id").and_then(|id| rels.get(&id).cloned()).or_else(|| attr(attrs, "w:anchor").map(|a| format!("#{a}")));
                    if let (Some(p), Some(href)) = (para.as_mut(), link.as_ref()) {
                        p.html.push_str(&format!("<a href=\"{}\">", escape(href)));
                    }
                }
                "a:blip" => {
                    let target = attr(attrs, "r:embed").and_then(|id| rels.get(&id).cloned());
                    if let (Some(p), Some(target)) = (para.as_mut(), target) {
                        let part_name = format!("word/{}", target.trim_start_matches('/').trim_start_matches("word/"));
                        if let Some(bytes) = part(&mut zip, &part_name) {
                            let ext = target.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
                            let mime = match ext.as_str() {
                                "jpg" | "jpeg" => "image/jpeg",
                                "gif" => "image/gif",
                                "bmp" => "image/bmp",
                                "svg" => "image/svg+xml",
                                _ => "image/png",
                            };
                            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                            p.html.push_str(&format!("<img src=\"data:{mime};base64,{data}\" alt=\"\" />"));
                        }
                    }
                }
                "w:tbl" => {
                    if table_depth == 0 {
                        close_lists(&mut body, &mut list_stack, 0);
                    }
                    table_depth += 1;
                    body.push_str("<table>\n");
                }
                "w:tr" => body.push_str("<tr>"),
                "w:tc" => {
                    body.push_str("<td>");
                    cell_paras = 0;
                }
                _ => {}
            },
            Token::Close { name } => match name.as_str() {
                "w:ppr" => in_ppr = false,
                "w:rpr" => in_rpr = false,
                "w:t" => in_text = false,
                "w:hyperlink" => {
                    if let (Some(p), Some(_)) = (para.as_mut(), link.take()) {
                        p.html.push_str("</a>");
                    }
                }
                "w:p" => {
                    let Some(p) = para.take() else { continue };
                    if table_depth > 0 {
                        if cell_paras > 0 {
                            body.push_str("<br />");
                        }
                        body.push_str(&p.html);
                        cell_paras += 1;
                        continue;
                    }
                    match p.list {
                        Some((num, level)) => {
                            let ordered = lists.get(&(num, level)).copied().unwrap_or(false);
                            let depth = level as usize + 1;
                            if list_stack.len() > depth {
                                close_lists(&mut body, &mut list_stack, depth);
                            }
                            if list_stack.len() == depth {
                                body.push_str("</li>\n<li>");
                            }
                            while list_stack.len() < depth {
                                body.push_str(if ordered { "<ol>\n<li>" } else { "<ul>\n<li>" });
                                list_stack.push((ordered, list_stack.len() as u8));
                            }
                            body.push_str(&p.html);
                        }
                        None => {
                            close_lists(&mut body, &mut list_stack, 0);
                            match p.heading {
                                Some(l) => body.push_str(&format!("<h{l}>{}</h{l}>\n", p.html)),
                                None if p.html.trim().is_empty() => body.push_str("<p><br /></p>\n"),
                                None => body.push_str(&format!("<p>{}</p>\n", p.html)),
                            }
                        }
                    }
                }
                "w:tc" => body.push_str("</td>"),
                "w:tr" => body.push_str("</tr>\n"),
                "w:tbl" => {
                    table_depth = table_depth.saturating_sub(1);
                    body.push_str("</table>\n");
                }
                _ => {}
            },
            Token::Text(s) if in_text => {
                if let Some(p) = para.as_mut() {
                    p.html.push_str(&run.wrap(&escape(&decode_entities(s))));
                }
            }
            Token::Text(_) => {}
        }
    }
    close_lists(&mut body, &mut list_stack, 0);
    Ok(crate::html::document(title, &body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_our_own_documents() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("t.docx");
        crate::docx::write(&out, &["Hello & <world>".into(), "Line one\nline two".into()], None).unwrap();
        let html = to_html(&out, "t").unwrap();
        assert!(html.contains("<p>Hello &amp; &lt;world&gt;</p>"), "{html}");
        assert!(html.contains("Line one</p>") || html.contains("Line one<br />line two"), "{html}");
    }
}
