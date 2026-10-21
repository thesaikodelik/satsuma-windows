//! Tools run with the choices of their options windows, on generated fixtures.

use satsuma_core::settings::{Settings, Strength};
use satsuma_core::{Format, Tool};
use satsuma_engine::plan::{CollageLayout, ToolOptions};
use satsuma_engine::probe::probe;
use satsuma_engine::{media, pdf, plan, run_job, Cancel, Context, Env, Operation};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn env() -> &'static Env {
    static ENV: OnceLock<Env> = OnceLock::new();
    ENV.get_or_init(Env::discover)
}

fn ffmpeg(args: &[&str]) {
    let status = Command::new(env().ffmpeg.as_ref().unwrap())
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "ffmpeg {args:?}");
}

fn apply(inputs: &[PathBuf], tool: Tool, options: ToolOptions) -> PathBuf {
    let settings = Settings::default();
    let cancel = Cancel::new();
    let ctx = Context { env: env(), settings: &settings, cancel: &cancel };
    let op = Operation::Configured(tool, options);
    let p = plan(env(), inputs, op.clone()).expect("plan");
    assert!(!p.jobs.is_empty(), "nothing to do for {op:?}: {:?}", p.skipped);
    run_job(&ctx, &p.jobs[0], &mut |_| {}).unwrap_or_else(|e| panic!("{op:?}: {e}"))
}

fn video(dir: &Path, seconds: u32) -> PathBuf {
    let p = dir.join("clip.mp4");
    ffmpeg(&[
        "-f", "lavfi", "-i", &format!("testsrc2=s=320x240:r=25:d={seconds}"),
        "-f", "lavfi", "-i", &format!("sine=f=440:d={seconds}"),
        "-pix_fmt", "yuv420p", "-shortest", p.to_str().unwrap(),
    ]);
    p
}

fn audio(dir: &Path, name: &str, channels: u32) -> PathBuf {
    let p = dir.join(name);
    ffmpeg(&["-f", "lavfi", "-i", "sine=f=330:d=3", "-ac", &channels.to_string(), p.to_str().unwrap()]);
    p
}

fn duration(p: &Path) -> f64 {
    probe(env(), p).unwrap().duration.unwrap()
}

fn have_ffmpeg() -> bool {
    if env().has_ffmpeg() {
        true
    } else {
        eprintln!("ffmpeg not installed; skipping");
        false
    }
}

#[test]
fn speed_snapshots_and_split() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let v = video(dir.path(), 4);

    let fast = apply(&[v.clone()], Tool::ChangeVideoSpeed, ToolOptions::Speed { factor: 2.0, keep_pitch: true });
    assert_eq!(fast.file_name().unwrap().to_string_lossy(), "clip Speed 2x.mp4");
    assert!((duration(&fast) - 2.0).abs() < 0.3, "{}", duration(&fast));
    let slow = apply(&[v.clone()], Tool::ChangeVideoSpeed, ToolOptions::Speed { factor: 0.5, keep_pitch: false });
    assert!((duration(&slow) - 8.0).abs() < 0.4, "{}", duration(&slow));

    let one = apply(&[v.clone()], Tool::VideoSnapshots, ToolOptions::Snapshots { times: vec![1.0], format: Format::Jpg });
    assert_eq!(one.extension().unwrap(), "jpg");
    let many = apply(&[v.clone()], Tool::VideoSnapshots, ToolOptions::Snapshots { times: vec![0.5, 1.5, 2.5], format: Format::Png });
    assert!(many.is_dir());
    assert_eq!(std::fs::read_dir(&many).unwrap().count(), 3);

    let clips = apply(&[v.clone()], Tool::SplitVideo, ToolOptions::Split { points: vec![1.0, 3.0] });
    let mut parts: Vec<PathBuf> = std::fs::read_dir(&clips).unwrap().map(|e| e.unwrap().path()).collect();
    parts.sort();
    assert_eq!(parts.len(), 3);
    assert!((duration(&parts[1]) - 2.0).abs() < 0.3);
}

#[test]
fn compress_to_a_size() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let v = dir.path().join("big.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=1280x720:r=30:d=6", "-pix_fmt", "yuv420p", "-c:v", "libx264", "-crf", "8", v.to_str().unwrap()]);
    let original = std::fs::metadata(&v).unwrap().len();
    let target = original / 4;
    let out = apply(&[v], Tool::Compress, ToolOptions::Compress { strength: Strength::Balanced, target_bytes: Some(target), max_edge: None });
    let got = std::fs::metadata(&out).unwrap().len();
    assert!(got <= target + target / 5, "{got} > {target}");

    let png = dir.path().join("photo.png");
    ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=1600x1200", "-frames:v", "1", png.to_str().unwrap()]);
    let jpg = dir.path().join("photo.jpg");
    ffmpeg(&["-i", png.to_str().unwrap(), "-q:v", "1", jpg.to_str().unwrap()]);
    let target = std::fs::metadata(&jpg).unwrap().len() / 3;
    let small = apply(&[jpg], Tool::Compress, ToolOptions::Compress { strength: Strength::Balanced, target_bytes: Some(target), max_edge: None });
    assert!(std::fs::metadata(&small).unwrap().len() <= target);
}

#[test]
fn channels_visualizer_and_tags() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mono = audio(dir.path(), "voice.mp3", 1);
    let stereo = apply(&[mono.clone()], Tool::AudioChannels, ToolOptions::Channels { stereo: true, left: 1.0, right: 0.3 });
    assert_eq!(stereo.file_name().unwrap().to_string_lossy(), "voice Stereo.mp3");
    assert_eq!(probe(env(), &stereo).unwrap().audio.unwrap().channels, 2);

    let square = apply(&[mono.clone()], Tool::AudioToVideo, ToolOptions::Visualize { width: 720, height: 720, still: false });
    assert_eq!(probe(env(), &square).unwrap().video, Some((720, 720)));
    let still = apply(&[mono.clone()], Tool::AudioToVideo, ToolOptions::Visualize { width: 1080, height: 1920, still: true });
    assert_eq!(probe(env(), &still).unwrap().video, Some((1080, 1920)));

    let tagged = apply(
        &[mono],
        Tool::RemoveMetadata,
        ToolOptions::Tags { clear: false, tags: vec![("title".into(), "Morning Song".into()), ("artist".into(), "Satsuma".into())] },
    );
    let tags = media::read_tags(env(), &tagged);
    assert!(tags.iter().any(|(k, v)| k == "title" && v == "Morning Song"), "{tags:?}");
}

#[test]
fn collage_row_and_pdf_info() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut images = Vec::new();
    for n in ["a.png", "b.png", "c.png"] {
        let p = dir.path().join(n);
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=s=300x200", "-frames:v", "1", p.to_str().unwrap()]);
        images.push(p);
    }
    let row = apply(&images, Tool::CreateCollage, ToolOptions::Collage { layout: CollageLayout::Row, cell: 200, gap: 10, background: [20, 20, 20] });
    let size = image::image_dimensions(&row).unwrap();
    assert_eq!(size, (3 * 200 + 2 * 10 + 2 * 10, 200 + 2 * 10));

    let pdf_in = dir.path().join("doc.pdf");
    let cancel = Cancel::new();
    satsuma_engine::image::images_to_pdf(env(), &images[..1], &pdf_in, dir.path(), &cancel, &mut |_| {}).unwrap();
    let out = apply(&[pdf_in], Tool::RemoveMetadata, ToolOptions::Tags { clear: true, tags: vec![("Title".into(), "Report — سلام".into())] });
    let info = pdf::read_info(&out);
    assert!(info.iter().any(|(k, v)| k == "Title" && v == "Report — سلام"), "{info:?}");
}

#[test]
fn trim_crop_and_bleep() {
    if !have_ffmpeg() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cancel = Cancel::new();
    let v = video(dir.path(), 4);
    let cut = dir.path().join("cut.mp4");
    media::trim(env(), &v, Format::Mp4, &cut, 1.0, 2.5, &cancel, &mut |_| {}).unwrap();
    assert!((duration(&cut) - 1.5).abs() < 0.25, "{}", duration(&cut));

    let cropped = dir.path().join("crop.mp4");
    media::crop_video(env(), &v, Format::Mp4, &cropped, (0.25, 0.25, 0.5, 0.5), &cancel, &mut |_| {}).unwrap();
    assert_eq!(probe(env(), &cropped).unwrap().video, Some((160, 120)));

    let a = audio(dir.path(), "talk.mp3", 2);
    let cut_audio = dir.path().join("talk cut.mp3");
    media::trim(env(), &a, Format::Mp3, &cut_audio, 0.5, 2.0, &cancel, &mut |_| {}).unwrap();
    assert!((duration(&cut_audio) - 1.5).abs() < 0.2);

    for tone in [false, true] {
        let out = dir.path().join(format!("bleep {tone}.mp3"));
        media::bleep(env(), &a, Format::Mp3, &out, &[(0.5, 1.0), (2.0, 2.2)], tone, &cancel, &mut |_| {}).unwrap();
        assert!((duration(&out) - 3.0).abs() < 0.2);
    }

    let peaks = media::peaks(env(), &a, 100).unwrap();
    assert_eq!(peaks.len(), 100);
    assert!(peaks.iter().all(|p| (0.0..=1.0).contains(p)));
}

#[test]
fn organize_pdf_pages() {
    let dir = tempfile::tempdir().unwrap();
    // Three pages of different widths, so the order can be read back.
    let mut parts = Vec::new();
    for (i, w) in [200u32, 300, 400].iter().enumerate() {
        let png = dir.path().join(format!("p{i}.png"));
        image::RgbaImage::from_pixel(*w, 100, image::Rgba([200, 100, 50, 255])).save(&png).unwrap();
        parts.push(png);
    }
    let pdf_in = dir.path().join("three.pdf");
    satsuma_engine::image::images_to_pdf(env(), &parts, &pdf_in, dir.path(), &Cancel::new(), &mut |_| {}).unwrap();
    let out = dir.path().join("organized.pdf");
    pdf::organize(&pdf_in, &out, &[(2, 0), (0, 90), (0, 0)]).unwrap();
    let doc = lopdf::Document::load(&out).unwrap();
    let pages: Vec<lopdf::ObjectId> = doc.get_pages().values().copied().collect();
    assert_eq!(pages.len(), 3);
    let width = |id| {
        let d = doc.get_object(id).unwrap().as_dict().unwrap();
        let mb = d.get(b"MediaBox").unwrap().as_array().unwrap();
        mb[2].as_float().unwrap() - mb[0].as_float().unwrap()
    };
    let rot = |id| doc.get_object(id).unwrap().as_dict().unwrap().get(b"Rotate").and_then(|r| r.as_i64()).unwrap_or(0);
    assert!(width(pages[0]) > width(pages[1]));
    assert_eq!(width(pages[1]), width(pages[2]));
    assert_eq!(rot(pages[1]), 90);
    assert_eq!(rot(pages[2]), 0);
    assert_ne!(pages[1], pages[2]);
}

#[test]
fn annotate_draws_over_the_photo() {
    let dir = tempfile::tempdir().unwrap();
    let photo = dir.path().join("photo.jpg");
    image::RgbImage::from_pixel(400, 300, image::Rgb([240, 240, 240])).save(&photo).unwrap();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300" viewBox="0 0 400 300"><rect x="50" y="50" width="100" height="80" fill="none" stroke="#ff0000" stroke-width="10"/><text x="200" y="200" font-size="40" fill="#0000ff">Hi</text></svg>"##;
    let out = apply(&[photo], Tool::AnnotateImage, ToolOptions::Annotate { svg: svg.into() });
    assert_eq!(out.extension().unwrap(), "png");
    let img = image::open(&out).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (400, 300));
    let p = img.get_pixel(50, 90).0;
    assert!(p[0] > 200 && p[1] < 60, "{p:?}");
    let blue = img.pixels().filter(|p| p.0[2] > 200 && p.0[0] < 80).count();
    assert!(blue > 50, "the text was drawn: {blue}");
}
