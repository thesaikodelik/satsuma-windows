//! The options windows of tools that ask before they run (speed, frames, split, compress,
//! channels, visualizer, collage, tags), built on [`super::form`].

use super::form::{Change, Field, Form};
use satsuma_core::settings::Strength;
use satsuma_core::{Category, Format, Tool};
use satsuma_engine::plan::{CollageLayout, ToolOptions};
use satsuma_engine::{detect_format, Operation};
use std::path::PathBuf;

fn env() -> &'static satsuma_engine::Env {
    satsuma_engine::win::env()
}

fn duration(path: &PathBuf) -> Option<f64> {
    satsuma_engine::probe::probe(env(), path).ok().and_then(|p| p.duration).filter(|d| *d > 0.0)
}

/// `1:05`, `12:30`, `1:02:03` (and tenths below ten seconds).
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0);
    if s < 10.0 && (s - s.round()).abs() >= 0.05 {
        return format!("0:{:04.1}", s);
    }
    let t = s.round() as u64;
    let (h, m, sec) = (t / 3600, (t / 60) % 60, t % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

pub fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb >= 100.0 {
        format!("{mb:.0} MB")
    } else if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1000.0)
    }
}

/// Whether `tool` asks for options on these files before it runs.
pub fn asks(tool: Tool, files: &[PathBuf]) -> bool {
    let Some(fmt) = files.first().and_then(|f| detect_format(f)) else { return false };
    match tool {
        Tool::ChangeVideoSpeed | Tool::VideoSnapshots | Tool::SplitVideo | Tool::Compress | Tool::AudioChannels | Tool::AudioToVideo | Tool::CreateCollage => true,
        Tool::RemoveMetadata => matches!(fmt.category(), Category::Audio | Category::Video) || fmt == Format::Pdf,
        Tool::TrimVideo | Tool::TrimAudio | Tool::CropVideo | Tool::OrganizePdf | Tool::RedactAudio | Tool::AnnotateImage | Tool::RedactImage | Tool::RedactVideo => true,
        _ => false,
    }
}

/// The options window for `tool` on `files`, when it has one.
pub fn form_for(tool: Tool, files: &[PathBuf]) -> Option<Box<dyn Form>> {
    let first = files.first()?.clone();
    let fmt = detect_format(&first)?;
    let files = files.to_vec();
    Some(match tool {
        Tool::ChangeVideoSpeed => Box::new(Speed { duration: duration(&first), files, factor: 2.0, keep_pitch: true }),
        Tool::VideoSnapshots => Box::new(Snapshots { duration: duration(&first)?, files, count: 1, jpg: false }),
        Tool::SplitVideo => Box::new(Split { duration: duration(&first)?, files, by_length: false, parts: 2, every: 0.0 }),
        Tool::Compress => {
            let size = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
            Box::new(Compress { category: if fmt == Format::Pdf { Category::Document } else { fmt.category() }, lossless: matches!(fmt, Format::Wav | Format::Flac | Format::Aiff | Format::Tiff), files, mode: 0, size, fraction: 0.5, edge: 0 })
        }
        Tool::AudioChannels => {
            let mono = satsuma_engine::probe::probe(env(), &first).ok().and_then(|p| p.audio).is_some_and(|a| a.channels == 1);
            Box::new(Channels { files, stereo: mono, left: 1.0, right: 1.0 })
        }
        Tool::AudioToVideo => Box::new(Visualize { files, shape: 0, still: false }),
        Tool::CreateCollage => Box::new(Collage { files, layout: 0, cell: 800.0, gap: 16.0, colour: 0 }),
        Tool::RemoveMetadata if matches!(fmt.category(), Category::Audio | Category::Video) || fmt == Format::Pdf => Box::new(Tags::new(files, fmt == Format::Pdf)),
        Tool::TrimVideo => Box::new(super::timeline::Trim::new(files, true)?),
        Tool::TrimAudio => Box::new(super::timeline::Trim::new(files, false)?),
        Tool::CropVideo => Box::new(super::cropvideo::CropVideo::new(files)?),
        Tool::OrganizePdf => Box::new(super::pages::Pages::new(files)?),
        Tool::RedactAudio => Box::new(super::bleep::Bleep::new(files)?),
        Tool::AnnotateImage => Box::new(super::annotate::Annotate::new(files)?),
        Tool::RedactImage => Box::new(super::redact::Redact::new(files)?),
        Tool::RedactVideo => Box::new(super::redactvideo::RedactVideo::new(files)?),
        _ => return None,
    })
}

// ---------------------------------------------------------------- speed

struct Speed {
    files: Vec<PathBuf>,
    duration: Option<f64>,
    factor: f64,
    keep_pitch: bool,
}

impl Form for Speed {
    fn title(&self) -> String {
        "Change Video Speed".into()
    }
    fn fields(&self) -> Vec<Field> {
        let mut v = vec![
            Field::Choice { id: 1, label: "Speed".into(), choices: ["0.5×", "1.5×", "2×", "4×"].map(String::from).to_vec(), value: [0.5, 1.5, 2.0, 4.0].iter().position(|f| (f - self.factor).abs() < 0.01).unwrap_or(usize::MAX) },
            Field::Slider { id: 2, label: "".into(), min: 0.25, max: 4.0, value: self.factor, text: format!("{:.2}×", self.factor) },
            Field::Toggle { id: 3, label: "Keep the pitch".into(), value: self.keep_pitch },
        ];
        if let Some(d) = self.duration {
            v.push(Field::Note(format!("{} becomes {}.", clock(d), clock(d / self.factor))));
        }
        v
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.factor = [0.5, 1.5, 2.0, 4.0][i.min(3)],
            (2, Change::Slider(v)) => self.factor = (v * 20.0).round() / 20.0,
            (3, Change::Toggle(b)) => self.keep_pitch = b,
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Change Speed".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        if (self.factor - 1.0).abs() < 0.01 {
            return Err("Choose a speed other than 1×.".into());
        }
        Ok(Operation::Configured(Tool::ChangeVideoSpeed, ToolOptions::Speed { factor: self.factor, keep_pitch: self.keep_pitch }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

// ---------------------------------------------------------------- frames

struct Snapshots {
    files: Vec<PathBuf>,
    duration: f64,
    count: u32,
    jpg: bool,
}

impl Snapshots {
    fn times(&self) -> Vec<f64> {
        let n = self.count.max(1);
        (0..n).map(|i| self.duration * (i as f64 + 0.5) / n as f64).collect()
    }
}

impl Form for Snapshots {
    fn title(&self) -> String {
        "Save Video Frames".into()
    }
    fn fields(&self) -> Vec<Field> {
        let times = self.times();
        let shown: Vec<String> = times.iter().take(6).map(|t| clock(*t)).collect();
        let more = if times.len() > 6 { format!(" and {} more", times.len() - 6) } else { String::new() };
        vec![
            Field::Slider { id: 1, label: "Frames".into(), min: 1.0, max: 30.0, value: self.count as f64, text: self.count.to_string() },
            Field::Choice { id: 2, label: "Format".into(), choices: vec!["PNG".into(), "JPG".into()], value: self.jpg as usize },
            Field::Note(format!("Evenly through the video, at {}{more}. Full resolution.", shown.join(", "))),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Slider(v)) => self.count = v.round().clamp(1.0, 30.0) as u32,
            (2, Change::Choice(i)) => self.jpg = i == 1,
            _ => {}
        }
    }
    fn action(&self) -> String {
        if self.count == 1 { "Save Frame".into() } else { format!("Save {} Frames", self.count) }
    }
    fn operation(&self) -> Result<Operation, String> {
        Ok(Operation::Configured(Tool::VideoSnapshots, ToolOptions::Snapshots { times: self.times(), format: if self.jpg { Format::Jpg } else { Format::Png } }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

// ---------------------------------------------------------------- split

struct Split {
    files: Vec<PathBuf>,
    duration: f64,
    by_length: bool,
    parts: u32,
    every: f64,
}

impl Split {
    fn every(&self) -> f64 {
        if self.every > 0.0 { self.every } else { (self.duration / 4.0).max(1.0).round() }
    }
    fn points(&self) -> Vec<f64> {
        if self.by_length {
            let step = self.every();
            let mut v = Vec::new();
            let mut t = step;
            while t < self.duration - 0.5 {
                v.push(t);
                t += step;
            }
            v
        } else {
            let n = self.parts.max(2);
            (1..n).map(|i| self.duration * i as f64 / n as f64).collect()
        }
    }
}

impl Form for Split {
    fn title(&self) -> String {
        "Split Video".into()
    }
    fn fields(&self) -> Vec<Field> {
        let mut v = vec![Field::Choice { id: 1, label: "Split".into(), choices: vec!["Into equal parts".into(), "By length".into()], value: self.by_length as usize }];
        if self.by_length {
            let max = (self.duration / 2.0).max(2.0);
            v.push(Field::Slider { id: 3, label: "Each clip".into(), min: 1.0, max, value: self.every().min(max), text: clock(self.every()) });
        } else {
            v.push(Field::Slider { id: 2, label: "Parts".into(), min: 2.0, max: 20.0, value: self.parts as f64, text: self.parts.to_string() });
        }
        let clips = self.points().len() + 1;
        v.push(Field::Note(format!("{clips} clips from {}, in one folder next to the video.", clock(self.duration))));
        v
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.by_length = i == 1,
            (2, Change::Slider(v)) => self.parts = v.round().clamp(2.0, 20.0) as u32,
            (3, Change::Slider(v)) => self.every = if v >= 20.0 { (v / 5.0).round() * 5.0 } else { v.round() }.max(1.0),
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Split".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        let points = self.points();
        if points.is_empty() {
            return Err("The clips would be as long as the video.".into());
        }
        Ok(Operation::Configured(Tool::SplitVideo, ToolOptions::Split { points }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

// ---------------------------------------------------------------- compress

struct Compress {
    files: Vec<PathBuf>,
    category: Category,
    lossless: bool,
    /// 0 Balanced, 1 Strong, 2 a size.
    mode: usize,
    size: u64,
    fraction: f64,
    /// 0 original, else index into EDGES.
    edge: usize,
}

const EDGES: [(&str, u32); 4] = [("Original", 0), ("4096 px", 4096), ("2048 px", 2048), ("1024 px", 1024)];

impl Compress {
    fn sized(&self) -> bool {
        !self.lossless && self.category != Category::Document
    }
    fn target(&self) -> u64 {
        (self.size as f64 * self.fraction) as u64
    }
}

impl Form for Compress {
    fn title(&self) -> String {
        "Compress".into()
    }
    fn fields(&self) -> Vec<Field> {
        let mut choices = vec!["Balanced".to_string(), "Strong".to_string()];
        if self.sized() {
            choices.push("To a size".into());
        }
        let mut v = vec![Field::Choice { id: 1, label: "Compression".into(), choices, value: self.mode }];
        if self.mode == 2 {
            v.push(Field::Slider { id: 2, label: "Size".into(), min: 0.05, max: 0.95, value: self.fraction, text: megabytes(self.target()) });
        }
        if self.category == Category::Image {
            v.push(Field::Choice { id: 3, label: "Longest edge".into(), choices: EDGES.iter().map(|e| e.0.to_string()).collect(), value: self.edge });
        }
        let what = if self.files.len() == 1 { "Now".to_string() } else { format!("{} files, now", self.files.len()) };
        let note = match self.mode {
            2 => format!("{what} {}; aims for about {} each in proportion.", megabytes(self.size), megabytes(self.target())),
            1 => format!("{what} {}. Strong makes smaller files with more quality loss.", megabytes(self.size)),
            _ => format!("{what} {}. Balanced keeps quality close to the original.", megabytes(self.size)),
        };
        v.push(Field::Note(note));
        v
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.mode = i,
            (2, Change::Slider(v)) => self.fraction = (v * 100.0).round() / 100.0,
            (3, Change::Choice(i)) => self.edge = i,
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Compress".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        let strength = if self.mode == 1 { Strength::Strong } else { Strength::Balanced };
        let max_edge = Some(EDGES[self.edge.min(EDGES.len() - 1)].1).filter(|e| *e > 0);
        let target_bytes = if self.mode == 2 {
            // With several files, each aims for the same share of its own size.
            let one = self.files.first().and_then(|f| std::fs::metadata(f).ok()).map(|m| m.len()).unwrap_or(self.size);
            Some((one as f64 * self.fraction) as u64)
        } else {
            None
        };
        Ok(Operation::Configured(Tool::Compress, ToolOptions::Compress { strength, target_bytes, max_edge }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

// ---------------------------------------------------------------- channels

struct Channels {
    files: Vec<PathBuf>,
    stereo: bool,
    left: f64,
    right: f64,
}

impl Form for Channels {
    fn title(&self) -> String {
        "Convert Audio Channels".into()
    }
    fn fields(&self) -> Vec<Field> {
        let pct = |v: f64| format!("{:.0}%", v * 100.0);
        vec![
            Field::Choice { id: 1, label: "Channels".into(), choices: vec!["Mono".into(), "Stereo".into()], value: self.stereo as usize },
            Field::Slider { id: 2, label: "Left volume".into(), min: 0.0, max: 2.0, value: self.left, text: pct(self.left) },
            Field::Slider { id: 3, label: "Right volume".into(), min: 0.0, max: 2.0, value: self.right, text: pct(self.right) },
            Field::Note(if self.stereo { "Each side plays at its own volume.".into() } else { "Both sides are mixed into one, at these volumes.".into() }),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        let snap = |v: f64| if (v - 1.0).abs() < 0.04 { 1.0 } else { (v * 20.0).round() / 20.0 };
        match (id, change) {
            (1, Change::Choice(i)) => self.stereo = i == 1,
            (2, Change::Slider(v)) => self.left = snap(v),
            (3, Change::Slider(v)) => self.right = snap(v),
            _ => {}
        }
    }
    fn action(&self) -> String {
        if self.stereo { "Make Stereo".into() } else { "Make Mono".into() }
    }
    fn operation(&self) -> Result<Operation, String> {
        Ok(Operation::Configured(Tool::AudioChannels, ToolOptions::Channels { stereo: self.stereo, left: self.left, right: self.right }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

// ---------------------------------------------------------------- visualizer

struct Visualize {
    files: Vec<PathBuf>,
    shape: usize,
    still: bool,
}

const SHAPES: [(&str, u32, u32); 3] = [("Landscape", 1920, 1080), ("Portrait", 1080, 1920), ("Square", 1080, 1080)];

impl Form for Visualize {
    fn title(&self) -> String {
        "Audio Visualizer".into()
    }
    fn fields(&self) -> Vec<Field> {
        let (_, w, h) = SHAPES[self.shape];
        vec![
            Field::Choice { id: 1, label: "Shape".into(), choices: SHAPES.iter().map(|s| s.0.to_string()).collect(), value: self.shape },
            Field::Choice { id: 2, label: "Picture".into(), choices: vec!["Moving waveform".into(), "Still waveform".into()], value: self.still as usize },
            Field::Note(format!("An MP4 video, {w} × {h}, with the sound as it is.")),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.shape = i.min(2),
            (2, Change::Choice(i)) => self.still = i == 1,
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Create Video".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        let (_, width, height) = SHAPES[self.shape];
        Ok(Operation::Configured(Tool::AudioToVideo, ToolOptions::Visualize { width, height, still: self.still }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

// ---------------------------------------------------------------- collage

struct Collage {
    files: Vec<PathBuf>,
    layout: usize,
    cell: f64,
    gap: f64,
    colour: usize,
}

const COLOURS: [[u8; 3]; 7] = [[255, 255, 255], [16, 17, 22], [245, 239, 230], [255, 110, 23], [37, 99, 235], [22, 163, 74], [124, 58, 237]];

impl Form for Collage {
    fn title(&self) -> String {
        "Create Collage".into()
    }
    fn fields(&self) -> Vec<Field> {
        vec![
            Field::Choice { id: 1, label: "Layout".into(), choices: vec!["Grid".into(), "Row".into(), "Column".into()], value: self.layout },
            Field::Slider { id: 2, label: "Picture size".into(), min: 200.0, max: 2000.0, value: self.cell, text: format!("{:.0} px", self.cell) },
            Field::Slider { id: 3, label: "Spacing".into(), min: 0.0, max: 80.0, value: self.gap, text: format!("{:.0} px", self.gap) },
            Field::Colours { id: 4, label: "Background".into(), colours: COLOURS.to_vec(), value: self.colour },
            Field::Note(format!("{} pictures, each fitted into a square, saved as a PNG.", self.files.len())),
        ]
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Choice(i)) => self.layout = i.min(2),
            (2, Change::Slider(v)) => self.cell = (v / 50.0).round() * 50.0,
            (3, Change::Slider(v)) => self.gap = v.round(),
            (4, Change::Colour(i)) => self.colour = i.min(COLOURS.len() - 1),
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Create Collage".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        let layout = [CollageLayout::Grid, CollageLayout::Row, CollageLayout::Column][self.layout];
        Ok(Operation::Configured(Tool::CreateCollage, ToolOptions::Collage { layout, cell: self.cell as u32, gap: self.gap as u32, background: COLOURS[self.colour] }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}

// ---------------------------------------------------------------- tags

struct Tags {
    files: Vec<PathBuf>,
    pdf: bool,
    /// (key written to the file, label, value)
    fields: Vec<(&'static str, &'static str, String)>,
    clear: bool,
}

impl Tags {
    fn new(files: Vec<PathBuf>, pdf: bool) -> Tags {
        let keys: Vec<(&'static str, &'static str)> = if pdf {
            vec![("Title", "Title"), ("Author", "Author"), ("Subject", "Subject"), ("Keywords", "Keywords")]
        } else {
            vec![("title", "Title"), ("artist", "Artist"), ("album", "Album"), ("date", "Year"), ("genre", "Genre"), ("comment", "Comment")]
        };
        let current: Vec<(String, String)> = match files.first() {
            Some(f) if pdf => satsuma_engine::pdf::read_info(f),
            Some(f) => satsuma_engine::media::read_tags(env(), f),
            None => Vec::new(),
        };
        let fields = keys
            .into_iter()
            .map(|(k, label)| {
                let v = current.iter().find(|(ck, _)| ck.eq_ignore_ascii_case(k)).map(|(_, v)| v.clone()).unwrap_or_default();
                (k, label, v)
            })
            .collect();
        Tags { files, pdf, fields, clear: false }
    }
}

impl Form for Tags {
    fn title(&self) -> String {
        "Edit Metadata".into()
    }
    fn fields(&self) -> Vec<Field> {
        let mut v: Vec<Field> = self
            .fields
            .iter()
            .enumerate()
            .map(|(i, (_, label, value))| Field::Text { id: 10 + i as u32, label: label.to_string(), value: value.clone(), placeholder: "None".into() })
            .collect();
        v.push(Field::Toggle { id: 1, label: "Remove everything else".into(), value: self.clear });
        v.push(Field::Note(if self.pdf {
            "Also removes hidden XMP data when switched on.".into()
        } else {
            "Dates, encoder details and cover pictures go too when switched on.".into()
        }));
        v
    }
    fn change(&mut self, id: u32, change: Change) {
        match (id, change) {
            (1, Change::Toggle(b)) => self.clear = b,
            (i, Change::Text(t)) if i >= 10 => {
                if let Some(f) = self.fields.get_mut((i - 10) as usize) {
                    f.2 = t;
                }
            }
            _ => {}
        }
    }
    fn action(&self) -> String {
        "Save".into()
    }
    fn operation(&self) -> Result<Operation, String> {
        let tags = self.fields.iter().map(|(k, _, v)| (k.to_string(), v.trim().to_string())).collect();
        Ok(Operation::Configured(Tool::RemoveMetadata, ToolOptions::Tags { clear: self.clear, tags }))
    }
    fn files(&self) -> Vec<PathBuf> {
        self.files.clone()
    }
}
