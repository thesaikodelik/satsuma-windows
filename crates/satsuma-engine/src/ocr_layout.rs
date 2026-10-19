//! Turning recognised text lines into paragraphs (shared by the macOS and Windows OCR backends).

/// One recognised line; the box is normalised with a top-left origin.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub x: f64,
    pub top: f64,
    pub height: f64,
}

/// Orders lines top to bottom and joins them into paragraphs at larger vertical gaps.
pub fn paragraphs(mut lines: Vec<Line>) -> Vec<String> {
    lines.retain(|l| !l.text.trim().is_empty());
    lines.sort_by(|a, b| a.top.partial_cmp(&b.top).unwrap().then(a.x.partial_cmp(&b.x).unwrap()));
    let mut heights: Vec<f64> = lines.iter().map(|l| l.height).collect();
    heights.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let typical = heights.get(heights.len() / 2).copied().unwrap_or(0.02);
    let mut out: Vec<String> = Vec::new();
    let mut prev_bottom: Option<f64> = None;
    for line in lines {
        let gap = prev_bottom.map(|b| line.top - b);
        let new_paragraph = match gap {
            None => true,
            Some(g) => g > typical * 0.9,
        };
        let text = line.text.trim();
        if new_paragraph || out.is_empty() {
            out.push(text.to_string());
        } else {
            let last = out.last_mut().unwrap();
            if last.ends_with('-') {
                last.pop();
            } else {
                last.push(' ');
            }
            last.push_str(text);
        }
        prev_bottom = Some(line.top + line.height);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, top: f64) -> Line {
        Line { text: text.into(), x: 0.1, top, height: 0.02 }
    }

    #[test]
    fn groups_lines_into_paragraphs() {
        let lines = vec![
            line("second para", 0.30),
            line("Title", 0.05),
            line("first para starts and con-", 0.12),
            line("tinues here", 0.145),
        ];
        assert_eq!(paragraphs(lines), vec!["Title", "first para starts and continues here", "second para"]);
    }
}
