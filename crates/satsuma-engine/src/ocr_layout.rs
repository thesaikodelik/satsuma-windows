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

/// One recognised word and its box, in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Lays a page's words out as a table: words at the same height form a row; wide gaps
/// split a row into cells, and cells starting near the same place share a column.
pub fn table_rows(mut words: Vec<Word>) -> Vec<Vec<String>> {
    words.retain(|w| !w.text.trim().is_empty());
    if words.is_empty() {
        return Vec::new();
    }
    let mut heights: Vec<f64> = words.iter().map(|w| w.h).collect();
    heights.sort_by(f64::total_cmp);
    let unit = heights[heights.len() / 2].max(1.0);

    // Rows: words whose middles sit within half a line of the row's.
    words.sort_by(|a, b| (a.y + a.h / 2.0).total_cmp(&(b.y + b.h / 2.0)));
    let mut rows: Vec<(f64, Vec<Word>)> = Vec::new();
    for w in words {
        let mid = w.y + w.h / 2.0;
        match rows.last_mut() {
            Some((centre, row)) if (mid - *centre).abs() <= unit * 0.5 => {
                row.push(w);
                *centre = row.iter().map(|w| w.y + w.h / 2.0).sum::<f64>() / row.len() as f64;
            }
            _ => rows.push((mid, vec![w])),
        }
    }

    // Cells: runs of words with gaps no wider than about a space.
    let cells: Vec<Vec<(f64, String)>> = rows
        .into_iter()
        .map(|(_, mut row)| {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            let mut out: Vec<(f64, f64, String)> = Vec::new();
            for w in row {
                match out.last_mut() {
                    Some((_, end, text)) if w.x - *end <= unit * 1.1 => {
                        text.push(' ');
                        text.push_str(w.text.trim());
                        *end = w.x + w.w;
                    }
                    _ => out.push((w.x, w.x + w.w, w.text.trim().to_string())),
                }
            }
            out.into_iter().map(|(x, _, t)| (x, t)).collect()
        })
        .collect();

    // Columns: where cells start, gathered into clusters across the page.
    let mut starts: Vec<f64> = cells.iter().flatten().map(|(x, _)| *x).collect();
    starts.sort_by(f64::total_cmp);
    let mut columns: Vec<Vec<f64>> = Vec::new();
    for x in starts {
        match columns.last_mut() {
            Some(c) if x - c[c.len() - 1] <= unit * 1.5 => c.push(x),
            _ => columns.push(vec![x]),
        }
    }
    let anchors: Vec<f64> = columns.iter().map(|c| c.iter().sum::<f64>() / c.len() as f64).collect();
    cells
        .into_iter()
        .map(|row| {
            let mut out = vec![String::new(); anchors.len()];
            for (x, text) in row {
                let col = anchors.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map(|(i, _)| i).unwrap_or(0);
                if !out[col].is_empty() {
                    out[col].push(' ');
                }
                out[col].push_str(&text);
            }
            while out.last().is_some_and(|c| c.is_empty()) {
                out.pop();
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f64, y: f64) -> Word {
        Word { text: text.into(), x, y, w: text.len() as f64 * 10.0, h: 20.0 }
    }

    #[test]
    fn words_become_a_table() {
        let words = vec![
            word("Name", 10.0, 10.0),
            word("Price", 300.0, 12.0),
            word("Green", 10.0, 50.0),
            word("tea", 66.0, 50.0),
            word("4.50", 302.0, 49.0),
            word("Total", 10.0, 90.0),
            word("9", 305.0, 91.0),
        ];
        let rows = table_rows(words);
        assert_eq!(rows, vec![vec!["Name", "Price"], vec!["Green tea", "4.50"], vec!["Total", "9"]]);
    }

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
