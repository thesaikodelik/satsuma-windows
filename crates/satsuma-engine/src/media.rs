//! Audio and video conversions and tools, implemented with ffmpeg.

use crate::env::Env;
use crate::probe::{probe, progress_fraction, Probe};
use crate::process::{run, Cancel};
use crate::{Error, Result};
use satsuma_core::settings::Strength;
use satsuma_core::Format::{self, *};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Keeps H.264/VP9/MPEG-4 happy with odd-sized sources.
const EVEN: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

/// How to get ffmpeg on this platform.
pub const FFMPEG_MISSING: &str = if cfg!(windows) {
    "ffmpeg is not installed. Install it with `winget install Gyan.FFmpeg`."
} else {
    "ffmpeg is not installed. Install it with `brew install ffmpeg`."
};

pub(crate) struct Ff<'a> {
    env: &'a Env,
    args: Vec<OsString>,
    duration: Option<f64>,
}

impl<'a> Ff<'a> {
    pub(crate) fn new(env: &'a Env) -> Self {
        Ff { env, args: Vec::new(), duration: None }
    }

    pub(crate) fn arg(mut self, a: impl Into<OsString>) -> Self {
        self.args.push(a.into());
        self
    }

    pub(crate) fn args<I, S>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(items.into_iter().map(Into::into));
        self
    }

    pub(crate) fn input(self, p: &Path) -> Self {
        self.arg("-i").arg(p)
    }

    pub(crate) fn duration(mut self, d: Option<f64>) -> Self {
        self.duration = d;
        self
    }

    /// Like [`Ff::run`] for numbered outputs (`%03d`); `first` must exist afterwards.
    pub(crate) fn run_pattern(self, pattern: &Path, first: &Path, cancel: &Cancel) -> Result<()> {
        let mut cmd = self.env.ffmpeg_command().ok_or_else(|| Error::msg(FFMPEG_MISSING))?;
        cmd.args(["-loglevel", "error", "-nostats"]);
        cmd.args(&self.args);
        cmd.arg(pattern);
        run(&mut cmd, cancel, &mut |_| {})?;
        if !first.exists() {
            return Err(Error::msg("ffmpeg finished without writing an output file."));
        }
        Ok(())
    }

    pub(crate) fn run(self, out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
        let mut cmd = self
            .env
            .ffmpeg_command()
            .ok_or_else(|| Error::msg(FFMPEG_MISSING))?;
        cmd.args(["-loglevel", "error", "-progress", "pipe:1", "-nostats"]);
        cmd.args(&self.args);
        cmd.arg(out);
        let duration = self.duration;
        run(&mut cmd, cancel, &mut |line| {
            if let Some(f) = progress_fraction(line, duration) {
                progress(f)
            }
        })?;
        if !out.exists() {
            return Err(Error::msg("ffmpeg finished without writing an output file."));
        }
        progress(1.0);
        Ok(())
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Audio encoder arguments for an output format.
pub fn audio_codec_args(env: &Env, target: Format, kbps: u32) -> Result<Vec<String>> {
    let br = format!("{kbps}k");
    let missing = |what: &str| Error::msg(format!("This ffmpeg build cannot encode {what}."));
    Ok(match target {
        Mp3 => {
            if !env.has_encoder("libmp3lame") {
                return Err(missing("MP3"));
            }
            vec!["-c:a".into(), "libmp3lame".into(), "-b:a".into(), br]
        }
        M4a | Mp4 | Mov | Mkv => {
            let enc = env.pick_encoder(&["aac", "aac_at"]).ok_or_else(|| missing("AAC"))?;
            vec!["-c:a".into(), enc.into(), "-b:a".into(), br]
        }
        Wav => s(&["-c:a", "pcm_s16le"]),
        Aiff => s(&["-c:a", "pcm_s16be"]),
        Flac => s(&["-c:a", "flac", "-compression_level", "8"]),
        Ogg => match env.pick_encoder(&["libvorbis", "vorbis"]) {
            Some("libvorbis") => s(&["-c:a", "libvorbis", "-q:a", "6"]),
            Some(_) => s(&["-c:a", "vorbis", "-strict", "experimental", "-ac", "2"]),
            None => return Err(missing("Ogg Vorbis")),
        },
        Opus | Webm => match env.pick_encoder(&["libopus", "opus"]) {
            Some("libopus") => vec!["-c:a".into(), "libopus".into(), "-b:a".into(), br],
            Some(_) => vec![
                "-c:a".into(),
                "opus".into(),
                "-strict".into(),
                "experimental".into(),
                "-ar".into(),
                "48000".into(),
                "-b:a".into(),
                br,
            ],
            None => return Err(missing("Opus")),
        },
        Wma | Wmv => {
            if !env.has_encoder("wmav2") {
                return Err(missing("WMA"));
            }
            vec!["-c:a".into(), "wmav2".into(), "-b:a".into(), br]
        }
        Avi => {
            if env.has_encoder("libmp3lame") {
                vec!["-c:a".into(), "libmp3lame".into(), "-b:a".into(), br]
            } else {
                s(&["-c:a", "pcm_s16le"])
            }
        }
        other => return Err(Error::msg(format!("{} is not an audio format.", other.label()))),
    })
}

fn h264_args(env: &Env, crf: u32) -> Result<Vec<String>> {
    if env.has_encoder("libx264") {
        Ok(vec![
            "-c:v".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
            "-crf".into(),
            crf.to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ])
    } else if env.has_encoder("h264_videotoolbox") {
        Ok(s(&["-c:v", "h264_videotoolbox", "-q:v", "65", "-pix_fmt", "yuv420p"]))
    } else if env.has_encoder("h264_mf") {
        // Media Foundation (Windows), quality-based rate control.
        let quality = (100 - crf.min(51) * 2).clamp(1, 100).to_string();
        Ok(vec!["-c:v".into(), "h264_mf".into(), "-rate_control".into(), "quality".into(), "-quality".into(), quality, "-pix_fmt".into(), "nv12".into()])
    } else {
        Err(Error::msg("This ffmpeg build cannot encode H.264 video."))
    }
}

/// Video (and audio) encoder arguments for a video container.
pub fn video_codec_args(env: &Env, target: Format) -> Result<Vec<String>> {
    let mut a = Vec::new();
    match target {
        Mp4 | Mov | Mkv => {
            a.extend(h264_args(env, 20)?);
            a.extend(audio_codec_args(env, target, 192)?);
            if target != Mkv {
                a.extend(s(&["-movflags", "+faststart"]));
            }
        }
        Webm => {
            if !env.has_encoder("libvpx-vp9") {
                return Err(Error::msg("This ffmpeg build cannot encode VP9 video."));
            }
            a.extend(s(&[
                "-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-row-mt", "1", "-deadline", "good",
                "-cpu-used", "4", "-pix_fmt", "yuv420p",
            ]));
            a.extend(audio_codec_args(env, Webm, 128)?);
        }
        Avi => {
            a.extend(s(&["-c:v", "mpeg4", "-q:v", "3", "-vtag", "xvid"]));
            a.extend(audio_codec_args(env, Avi, 192)?);
        }
        Wmv => {
            if !env.has_encoder("wmv2") {
                return Err(Error::msg("This ffmpeg build cannot encode WMV video."));
            }
            a.extend(s(&["-c:v", "wmv2", "-b:v", "5M"]));
            a.extend(audio_codec_args(env, Wmv, 192)?);
        }
        other => return Err(Error::msg(format!("{} is not a video format.", other.label()))),
    }
    Ok(a)
}

fn probe_opt(env: &Env, input: &Path) -> Probe {
    probe(env, input).unwrap_or_default()
}

/// audio → audio
pub fn convert_audio(env: &Env, input: &Path, out: &Path, target: Format, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    Ff::new(env)
        .input(input)
        .args(["-map", "0:a:0", "-vn", "-map_metadata", "0"])
        .args(audio_codec_args(env, target, 192)?)
        .duration(p.duration)
        .run(out, cancel, progress)
}

/// video/gif → video, gif or mp3
pub fn convert_video(env: &Env, input: &Path, out: &Path, target: Format, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    let ff = Ff::new(env).input(input).duration(p.duration);
    let ff = match target {
        Gif => ff
            .args(["-map", "0:v:0", "-an", "-vf"])
            .arg("fps=12,scale='min(640,iw)':-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4")
            .args(["-loop", "0"]),
        Mp3 => ff
            .args(["-map", "0:a:0", "-vn"])
            .args(audio_codec_args(env, Mp3, 192)?),
        _ => ff
            .args(["-map", "0:v:0", "-map", "0:a:0?", "-vf", EVEN])
            .args(video_codec_args(env, target)?),
    };
    if target == Mp3 && p.audio.is_none() && p.duration.is_some() {
        return Err(Error::msg("This video has no audio track."));
    }
    ff.run(out, cancel, progress)
}

fn compress_kbps(strength: Strength) -> u32 {
    match strength {
        Strength::Balanced => 128,
        Strength::Strong => 96,
    }
}

/// Compress audio (mp3, m4a: lower bitrate; wav: lossless FLAC; flac: max compression).
pub fn compress_audio(env: &Env, input: &Path, in_fmt: Format, out: &Path, strength: Strength, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    let codec = match in_fmt {
        Wav => s(&["-c:a", "flac", "-compression_level", "8"]),
        Flac => s(&["-c:a", "flac", "-compression_level", "12"]),
        other => audio_codec_args(env, other, compress_kbps(strength))?,
    };
    Ff::new(env)
        .input(input)
        .args(["-map", "0:a:0", "-vn", "-map_metadata", "0"])
        .args(codec)
        .duration(p.duration)
        .run(out, cancel, progress)
}

/// Compress video with HEVC (VideoToolbox when available).
pub fn compress_video(env: &Env, input: &Path, in_fmt: Format, out: &Path, strength: Strength, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    let strong = strength == Strength::Strong;
    let mut v: Vec<String> = if env.has_encoder("hevc_videotoolbox") {
        vec!["-c:v".into(), "hevc_videotoolbox".into(), "-q:v".into(), (if strong { "38" } else { "50" }).into()]
    } else if env.has_encoder("libx265") {
        vec!["-c:v".into(), "libx265".into(), "-preset".into(), "fast".into(), "-crf".into(), (if strong { "32" } else { "28" }).into()]
    } else {
        h264_args(env, if strong { 30 } else { 26 })?
    };
    if !v.iter().any(|a| a == "-pix_fmt") {
        v.extend(s(&["-pix_fmt", "yuv420p"]));
    }
    if in_fmt != Mkv && v.iter().any(|a| a.contains("hevc") || a.contains("265")) {
        v.extend(s(&["-tag:v", "hvc1"]));
    }
    let audio = audio_codec_args(env, Mp4, if strong { 96 } else { 128 })?;
    let mut ff = Ff::new(env)
        .input(input)
        .args(["-map", "0:v:0", "-map", "0:a:0?", "-vf", EVEN])
        .args(v)
        .args(audio)
        .duration(p.duration);
    if in_fmt != Mkv {
        ff = ff.args(["-movflags", "+faststart"]);
    }
    ff.run(out, cancel, progress)
}

/// Strip container and stream metadata without re-encoding.
pub fn remove_metadata(env: &Env, input: &Path, in_fmt: Format, out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let mut ff = Ff::new(env)
        .input(input)
        .args(["-map", "0", "-map_metadata", "-1", "-map_metadata:s", "-1", "-map_chapters", "-1"])
        .args(["-c", "copy", "-fflags", "+bitexact", "-flags:v", "+bitexact", "-flags:a", "+bitexact"]);
    if matches!(in_fmt, Mp4 | Mov | M4a) {
        ff = ff.args(["-movflags", "+faststart"]);
    }
    if in_fmt == Mp3 {
        ff = ff.args(["-write_xing", "0", "-id3v2_version", "0"]);
    }
    if in_fmt == Gif {
        return Err(Error::msg("GIF metadata is removed without ffmpeg."));
    }
    ff.run(out, cancel, progress)
}

fn audio_filter_tool(env: &Env, input: &Path, in_fmt: Format, out: &Path, extra: Vec<String>, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    Ff::new(env)
        .input(input)
        .args(["-map", "0:a:0", "-vn", "-map_metadata", "0"])
        .args(extra)
        .args(audio_codec_args(env, in_fmt, 192)?)
        .duration(p.duration)
        .run(out, cancel, progress)
}

/// EBU R128 loudness normalisation to −16 LUFS.
pub fn normalize(env: &Env, input: &Path, in_fmt: Format, out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let rate = probe_opt(env, input).audio.map(|a| a.sample_rate).unwrap_or(48_000);
    audio_filter_tool(
        env,
        input,
        in_fmt,
        out,
        vec!["-af".into(), "loudnorm=I=-16:LRA=11:TP=-1.5".into(), "-ar".into(), rate.to_string()],
        cancel,
        progress,
    )
}

/// Remove silence at the start and end only, keeping pauses inside.
pub fn trim_silence(env: &Env, input: &Path, in_fmt: Format, out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let edge = "silenceremove=start_periods=1:start_duration=0:start_threshold=-50dB:start_silence=0.05";
    let filter = format!("{edge},areverse,{edge},areverse");
    audio_filter_tool(env, input, in_fmt, out, vec!["-af".into(), filter], cancel, progress)
}

/// Mono ↔ stereo. Returns the channel count written.
pub fn change_channels(env: &Env, input: &Path, in_fmt: Format, out: &Path, channels: u32, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    audio_filter_tool(env, input, in_fmt, out, vec!["-ac".into(), channels.to_string()], cancel, progress)
}

/// The channel count Convert Audio Channels will produce: mono, or stereo for mono input.
pub fn target_channels(env: &Env, input: &Path) -> u32 {
    match probe_opt(env, input).audio {
        Some(a) if a.channels == 1 => 2,
        _ => 1,
    }
}

pub fn mute(env: &Env, input: &Path, in_fmt: Format, out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    let mut ff = Ff::new(env)
        .input(input)
        .args(["-map", "0:v:0", "-c:v", "copy", "-an", "-map_metadata", "0"])
        .duration(p.duration);
    if in_fmt != Mkv {
        ff = ff.args(["-movflags", "+faststart"]);
    }
    ff.run(out, cancel, progress)
}

pub fn change_speed(env: &Env, input: &Path, in_fmt: Format, out: &Path, factor: f64, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    let mut ff = Ff::new(env)
        .input(input)
        .arg("-filter:v")
        .arg(format!("setpts=PTS/{factor},{EVEN}"))
        .args(["-map", "0:v:0"]);
    if p.audio.is_some() {
        ff = ff.args(["-map", "0:a:0", "-filter:a"]).arg(atempo_chain(factor));
    }
    ff.args(video_codec_args(env, in_fmt)?)
        .duration(p.duration.map(|d| d / factor))
        .run(out, cancel, progress)
}

/// `atempo` only accepts 0.5–2.0 per stage, so chain stages for other factors.
pub fn atempo_chain(mut factor: f64) -> String {
    let mut stages = Vec::new();
    while factor > 2.0 {
        stages.push("atempo=2.0".to_string());
        factor /= 2.0;
    }
    while factor < 0.5 {
        stages.push("atempo=0.5".to_string());
        factor /= 0.5;
    }
    stages.push(format!("atempo={factor}"));
    stages.join(",")
}

/// Full-resolution PNG of the middle frame.
pub fn snapshot(env: &Env, input: &Path, out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe(env, input)?;
    let at = p.duration.unwrap_or(0.0) / 2.0;
    Ff::new(env)
        .arg("-ss")
        .arg(format!("{at:.3}"))
        .input(input)
        .args(["-map", "0:v:0", "-frames:v", "1", "-update", "1"])
        .run(out, cancel, progress)
}

/// Split into `parts` equal clips inside `out_dir`.
pub fn split_equal(env: &Env, input: &Path, in_fmt: Format, out_dir: &Path, stem: &str, parts: u32, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<Vec<PathBuf>> {
    let p = probe(env, input)?;
    let duration = p.duration.ok_or_else(|| Error::msg("Could not read the video duration."))?;
    let parts = parts.max(2);
    let len = duration / parts as f64;
    if len < 0.05 {
        return Err(Error::msg("The video is too short to split."));
    }
    std::fs::create_dir_all(out_dir)?;
    let mut outputs = Vec::new();
    for i in 0..parts {
        let out = out_dir.join(format!("{stem} {:02}.{}", i + 1, in_fmt.extension()));
        let start = len * i as f64;
        let base = i as f64 / parts as f64;
        let mut ff = Ff::new(env)
            .arg("-ss")
            .arg(format!("{start:.3}"))
            .input(input)
            .arg("-t")
            .arg(format!("{len:.3}"))
            .args(["-map", "0:v:0", "-map", "0:a:0?", "-vf", EVEN])
            .args(video_codec_args(env, in_fmt)?)
            .duration(Some(len));
        if in_fmt != Mkv {
            ff = ff.args(["-avoid_negative_ts", "make_zero"]);
        }
        ff.run(&out, cancel, &mut |f| progress(base + f / parts as f64))?;
        outputs.push(out);
    }
    Ok(outputs)
}

/// Join clips into one MP4; the first clip sets the canvas, others are letterboxed.
pub fn join(env: &Env, inputs: &[PathBuf], out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    if inputs.len() < 2 {
        return Err(Error::msg("Select at least two videos to join."));
    }
    let probes: Vec<Probe> = inputs.iter().map(|i| probe(env, i)).collect::<Result<_>>()?;
    if probes.iter().any(|p| p.video.is_none()) {
        return Err(Error::msg("Every selected file must contain a video track."));
    }
    let (w, h) = probes[0].video.unwrap();
    let (w, h) = (w / 2 * 2, h / 2 * 2);
    let total: f64 = probes.iter().filter_map(|p| p.duration).sum();

    let mut ff = Ff::new(env);
    for i in inputs {
        ff = ff.input(i);
    }
    // Silent audio for clips without an audio track.
    let mut silent_index = inputs.len();
    let mut graph = String::new();
    let mut concat_inputs = String::new();
    for (k, p) in probes.iter().enumerate() {
        graph.push_str(&format!(
            "[{k}:v:0]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p[v{k}];"
        ));
        if p.audio.is_some() {
            graph.push_str(&format!(
                "[{k}:a:0]aformat=sample_rates=48000:channel_layouts=stereo[a{k}];"
            ));
        } else {
            let d = p.duration.unwrap_or(1.0);
            ff = ff
                .args(["-f", "lavfi", "-t"])
                .arg(format!("{d:.3}"))
                .args(["-i", "anullsrc=r=48000:cl=stereo"]);
            graph.push_str(&format!("[{silent_index}:a:0]anull[a{k}];"));
            silent_index += 1;
        }
        concat_inputs.push_str(&format!("[v{k}][a{k}]"));
    }
    graph.push_str(&format!(
        "{concat_inputs}concat=n={}:v=1:a=1[v][a]",
        inputs.len()
    ));
    ff.arg("-filter_complex")
        .arg(graph)
        .args(["-map", "[v]", "-map", "[a]"])
        .args(h264_args(env, 20)?)
        .args(audio_codec_args(env, Mp4, 192)?)
        .args(["-movflags", "+faststart"])
        .duration(Some(total))
        .run(out, cancel, progress)
}

/// MP4 with an orange waveform on black.
pub fn visualize(env: &Env, input: &Path, out: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe(env, input)?;
    Ff::new(env)
        .input(input)
        .arg("-filter_complex")
        .arg("[0:a:0]showwaves=s=1280x720:mode=cline:rate=30:colors=0xFF7A1A:scale=sqrt,format=yuv420p[v]")
        .args(["-map", "[v]", "-map", "0:a:0"])
        .args(h264_args(env, 23)?)
        .args(audio_codec_args(env, Mp4, 192)?)
        .args(["-shortest", "-movflags", "+faststart"])
        .duration(p.duration)
        .run(out, cancel, progress)
}


// ---------------------------------------------------------------- tools with options

/// Change speed by `factor`. `keep_pitch` stretches the sound in time; otherwise it plays
/// faster or slower like tape, higher or lower.
#[allow(clippy::too_many_arguments)]
pub fn change_speed_with(env: &Env, input: &Path, in_fmt: Format, out: &Path, factor: f64, keep_pitch: bool, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    if !(0.1..=16.0).contains(&factor) {
        return Err(Error::msg("Choose a speed between 0.1x and 16x."));
    }
    let p = probe_opt(env, input);
    let mut ff = Ff::new(env)
        .input(input)
        .arg("-filter:v")
        .arg(format!("setpts=PTS/{factor},{EVEN}"))
        .args(["-map", "0:v:0"]);
    if let Some(a) = &p.audio {
        let filter = if keep_pitch {
            atempo_chain(factor)
        } else {
            format!("asetrate={},aresample={}", (a.sample_rate as f64 * factor).round(), a.sample_rate)
        };
        ff = ff.args(["-map", "0:a:0", "-filter:a"]).arg(filter);
    }
    ff.args(video_codec_args(env, in_fmt)?)
        .duration(p.duration.map(|d| d / factor))
        .run(out, cancel, progress)
}

/// One full-resolution frame at `at` seconds, as the image `out` (PNG or JPG by extension).
pub fn frame_to(env: &Env, input: &Path, at: f64, out: &Path, cancel: &Cancel) -> Result<()> {
    let mut ff = Ff::new(env)
        .arg("-ss")
        .arg(format!("{:.3}", at.max(0.0)))
        .input(input)
        .args(["-map", "0:v:0", "-frames:v", "1", "-update", "1"]);
    if out.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg")) {
        ff = ff.args(["-q:v", "2"]);
    }
    ff.run(out, cancel, &mut |_| {})
}

/// Frames at `times`: one image at `out`, or numbered images in the folder `out`.
#[allow(clippy::too_many_arguments)]
pub fn frames(env: &Env, input: &Path, times: &[f64], format: Format, out: &Path, stem: &str, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    if times.is_empty() {
        return Err(Error::msg("Choose at least one frame."));
    }
    if times.len() == 1 {
        return frame_to(env, input, times[0], out, cancel);
    }
    std::fs::create_dir_all(out)?;
    for (i, t) in times.iter().enumerate() {
        cancel.check()?;
        let file = out.join(format!("{stem} {:02}.{}", i + 1, format.extension()));
        frame_to(env, input, *t, &file, cancel)?;
        progress((i + 1) as f64 / times.len() as f64);
    }
    Ok(())
}

/// Split at `points` (seconds) into clips inside `out_dir`.
#[allow(clippy::too_many_arguments)]
pub fn split_at(env: &Env, input: &Path, in_fmt: Format, out_dir: &Path, stem: &str, points: &[f64], cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<Vec<PathBuf>> {
    let p = probe(env, input)?;
    let duration = p.duration.ok_or_else(|| Error::msg("Could not read the video duration."))?;
    let mut cuts: Vec<f64> = points.iter().copied().filter(|t| *t > 0.05 && *t < duration - 0.05).collect();
    cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cuts.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    if cuts.is_empty() {
        return Err(Error::msg("Choose where to split the video."));
    }
    let mut bounds = vec![0.0];
    bounds.extend(cuts);
    bounds.push(duration);
    let parts = bounds.len() - 1;
    std::fs::create_dir_all(out_dir)?;
    let mut outputs = Vec::new();
    for i in 0..parts {
        let (start, len) = (bounds[i], bounds[i + 1] - bounds[i]);
        let out = out_dir.join(format!("{stem} {:02}.{}", i + 1, in_fmt.extension()));
        let base = start / duration;
        let mut ff = Ff::new(env)
            .arg("-ss")
            .arg(format!("{start:.3}"))
            .input(input)
            .arg("-t")
            .arg(format!("{len:.3}"))
            .args(["-map", "0:v:0", "-map", "0:a:0?", "-vf", EVEN])
            .args(video_codec_args(env, in_fmt)?)
            .duration(Some(len));
        if in_fmt != Mkv {
            ff = ff.args(["-avoid_negative_ts", "make_zero"]);
        }
        ff.run(&out, cancel, &mut |f| progress(base + f * len / duration))?;
        outputs.push(out);
    }
    Ok(outputs)
}

/// Compress a video to about `target` bytes (H.264 at the bitrate that fits).
pub fn compress_video_to(env: &Env, input: &Path, in_fmt: Format, out: &Path, target: u64, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe(env, input)?;
    let duration = p.duration.filter(|d| *d > 0.1).ok_or_else(|| Error::msg("Could not read the video duration."))?;
    let audio_kbps: i64 = if p.audio.is_some() { 96 } else { 0 };
    let total_kbps = (target as f64 * 8.0 / 1000.0 / duration * 0.95) as i64;
    let video_kbps = total_kbps - audio_kbps;
    if video_kbps < 60 {
        return Err(Error::msg("That size is too small for this video. Choose a larger one."));
    }
    let mut v: Vec<String> = if env.has_encoder("libx264") {
        s(&["-c:v", "libx264", "-preset", "medium"])
    } else if env.has_encoder("h264_mf") {
        s(&["-c:v", "h264_mf", "-rate_control", "cbr"])
    } else {
        return Err(Error::msg("This ffmpeg build cannot encode H.264 video."));
    };
    v.extend([
        "-b:v".to_string(),
        format!("{video_kbps}k"),
        "-maxrate".to_string(),
        format!("{}k", video_kbps * 3 / 2),
        "-bufsize".to_string(),
        format!("{}k", video_kbps * 2),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
    ]);
    let mut ff = Ff::new(env)
        .input(input)
        .args(["-map", "0:v:0", "-map", "0:a:0?", "-vf", EVEN])
        .args(v)
        .args(audio_codec_args(env, Mp4, 96)?)
        .duration(Some(duration));
    if in_fmt != Mkv {
        ff = ff.args(["-movflags", "+faststart"]);
    }
    ff.run(out, cancel, progress)
}

/// Compress audio to about `target` bytes (MP3, M4A and the like, at the bitrate that fits).
pub fn compress_audio_to(env: &Env, input: &Path, in_fmt: Format, out: &Path, target: u64, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    if matches!(in_fmt, Wav | Flac | Aiff) {
        return Err(Error::msg("Lossless audio can't be squeezed to a size; choose Balanced or Strong."));
    }
    let p = probe(env, input)?;
    let duration = p.duration.filter(|d| *d > 0.1).ok_or_else(|| Error::msg("Could not read the audio duration."))?;
    let kbps = (target as f64 * 8.0 / 1000.0 / duration * 0.97) as i64;
    if kbps < 24 {
        return Err(Error::msg("That size is too small for this audio. Choose a larger one."));
    }
    Ff::new(env)
        .input(input)
        .args(["-map", "0:a:0", "-vn", "-map_metadata", "0"])
        .args(audio_codec_args(env, in_fmt, kbps.min(320) as u32)?)
        .duration(Some(duration))
        .run(out, cancel, progress)
}

/// Mono or stereo, with each side's volume (1.0 = unchanged).
#[allow(clippy::too_many_arguments)]
pub fn mix_channels(env: &Env, input: &Path, in_fmt: Format, out: &Path, stereo: bool, left: f64, right: f64, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let mono_in = probe_opt(env, input).audio.is_some_and(|a| a.channels == 1);
    let pan = match (stereo, mono_in) {
        (true, true) => format!("pan=stereo|c0={left:.3}*c0|c1={right:.3}*c0"),
        (true, false) => format!("pan=stereo|c0={left:.3}*c0|c1={right:.3}*c1"),
        (false, true) => format!("pan=mono|c0={:.3}*c0", (left + right) / 2.0),
        (false, false) => format!("pan=mono|c0={:.3}*c0+{:.3}*c1", left * 0.5, right * 0.5),
    };
    audio_filter_tool(env, input, in_fmt, out, vec!["-af".into(), pan], cancel, progress)
}

/// MP4 of the audio with a `width` × `height` picture: a moving waveform, or a still one.
#[allow(clippy::too_many_arguments)]
pub fn visualize_with(env: &Env, input: &Path, out: &Path, width: u32, height: u32, still: bool, tmp: &Path, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe(env, input)?;
    let (w, h) = (width.max(64) / 2 * 2, height.max(64) / 2 * 2);
    if still {
        let picture = crate::image::unique_in(tmp, "waveform.png");
        Ff::new(env)
            .input(input)
            .arg("-filter_complex")
            .arg(format!("[0:a:0]showwavespic=s={w}x{}:colors=0xFF7A1A:scale=sqrt,pad={w}:{h}:0:(oh-ih)/2:color=black[v]", h * 2 / 3 / 2 * 2))
            .args(["-map", "[v]", "-frames:v", "1"])
            .run(&picture, cancel, &mut |_| {})?;
        let mut v = h264_args(env, 23)?;
        if v.iter().any(|a| a == "libx264") {
            v.extend(s(&["-tune", "stillimage"]));
        }
        return Ff::new(env)
            .args(["-loop", "1", "-framerate", "2"])
            .input(&picture)
            .input(input)
            .args(["-map", "0:v:0", "-map", "1:a:0"])
            .args(v)
            .args(audio_codec_args(env, Mp4, 192)?)
            .args(["-shortest", "-movflags", "+faststart"])
            .duration(p.duration)
            .run(out, cancel, progress);
    }
    Ff::new(env)
        .input(input)
        .arg("-filter_complex")
        .arg(format!("[0:a:0]showwaves=s={w}x{h}:mode=cline:rate=30:colors=0xFF7A1A:scale=sqrt,format=yuv420p[v]"))
        .args(["-map", "[v]", "-map", "0:a:0"])
        .args(h264_args(env, 23)?)
        .args(audio_codec_args(env, Mp4, 192)?)
        .args(["-shortest", "-movflags", "+faststart"])
        .duration(p.duration)
        .run(out, cancel, progress)
}

/// Set tags such as title and artist without re-encoding. `clear` removes all others first.
#[allow(clippy::too_many_arguments)]
pub fn set_tags(env: &Env, input: &Path, in_fmt: Format, out: &Path, clear: bool, tags: &[(String, String)], cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe_opt(env, input);
    let mut ff = Ff::new(env)
        .input(input)
        .args(["-map", "0", "-c", "copy", "-map_metadata", if clear { "-1" } else { "0" }]);
    for (k, v) in tags {
        ff = ff.arg("-metadata").arg(format!("{k}={v}"));
    }
    if matches!(in_fmt, Mp4 | Mov | M4a) {
        ff = ff.args(["-movflags", "+faststart+use_metadata_tags"]);
    }
    if in_fmt == Mp3 {
        ff = ff.args(["-id3v2_version", "3"]);
    }
    ff.duration(p.duration).run(out, cancel, progress)
}

/// The tags a file carries now (title, artist and so on), read with ffmpeg.
pub fn read_tags(env: &Env, input: &Path) -> Vec<(String, String)> {
    let Some(mut cmd) = env.ffmpeg_command() else { return Vec::new() };
    cmd.args(["-hide_banner", "-nostdin", "-i"]).arg(input).args(["-f", "ffmetadata", "-"]);
    let Ok((_, stdout, _)) = crate::process::capture(&mut cmd) else { return Vec::new() };
    stdout
        .lines()
        .filter(|l| !l.starts_with(';') && !l.starts_with('['))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect()
}

/// Keep `start`..`end` seconds of an audio or video file.
pub fn trim(env: &Env, input: &Path, in_fmt: Format, out: &Path, start: f64, end: f64, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let len = end - start;
    if len < 0.05 {
        return Err(Error::msg("Choose a longer part to keep."));
    }
    let mut ff = Ff::new(env)
        .arg("-ss")
        .arg(format!("{:.3}", start.max(0.0)))
        .input(input)
        .arg("-t")
        .arg(format!("{len:.3}"))
        .args(["-map_metadata", "0"])
        .duration(Some(len));
    if in_fmt.category() == satsuma_core::Category::Video {
        ff = ff.args(["-map", "0:v:0", "-map", "0:a:0?", "-vf", EVEN]).args(video_codec_args(env, in_fmt)?);
        if in_fmt != Mkv {
            ff = ff.args(["-avoid_negative_ts", "make_zero"]);
        }
    } else {
        ff = ff.args(["-map", "0:a:0", "-vn"]).args(audio_codec_args(env, in_fmt, 192)?);
    }
    ff.run(out, cancel, progress)
}

/// Crop a video to a rectangle given as fractions of the picture.
#[allow(clippy::too_many_arguments)]
pub fn crop_video(env: &Env, input: &Path, in_fmt: Format, out: &Path, rect: (f64, f64, f64, f64), cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let p = probe(env, input)?;
    let (vw, vh) = p.video.ok_or_else(|| Error::msg("This file has no picture to crop."))?;
    let even = |v: f64| ((v.round() as i64).max(2) / 2 * 2) as u32;
    // Keep the size; move the box back inside the picture if rounding pushed it out.
    let w = even(rect.2 * vw as f64).min(vw / 2 * 2);
    let h = even(rect.3 * vh as f64).min(vh / 2 * 2);
    let x = (even(rect.0 * vw as f64)).min(vw - w) / 2 * 2;
    let y = (even(rect.1 * vh as f64)).min(vh - h) / 2 * 2;
    if w < 16 || h < 16 {
        return Err(Error::msg("The crop is too small."));
    }
    Ff::new(env)
        .input(input)
        .args(["-map", "0:v:0", "-map", "0:a:0?", "-map_metadata", "0", "-vf"])
        .arg(format!("crop={w}:{h}:{x}:{y}"))
        .args(video_codec_args(env, in_fmt)?)
        .duration(p.duration)
        .run(out, cancel, progress)
}

/// Hides boxes — (x, y, width, height) as fractions — by blurring, pixelating or filling
/// them, through the whole clip or from `from` to `to` seconds.
#[allow(clippy::too_many_arguments)]
pub fn redact_video(
    env: &Env,
    input: &Path,
    in_fmt: Format,
    out: &Path,
    boxes: &[[f64; 4]],
    style: crate::plan::RedactStyle,
    from: f64,
    to: Option<f64>,
    cancel: &Cancel,
    progress: &mut dyn FnMut(f64),
) -> Result<()> {
    use crate::plan::RedactStyle;
    if boxes.is_empty() {
        return Err(Error::msg("Mark at least one area to hide."));
    }
    let p = probe(env, input)?;
    let (vw, vh) = p.video.ok_or_else(|| Error::msg("This file has no picture to redact."))?;
    let when = match to {
        Some(t) => format!(":enable='between(t,{from:.3},{t:.3})'"),
        None if from > 0.0 => format!(":enable='gte(t,{from:.3})'"),
        None => String::new(),
    };
    let block = (vw.max(vh) / 70).max(6);
    let even = |v: f64| ((v.round() as i64).max(0) / 2 * 2) as u32;
    let mut graph = String::from("[0:v:0]null[v0]");
    let mut n = 0;
    for b in boxes {
        let w = even(b[2].clamp(0.0, 1.0) * vw as f64).clamp(4, vw / 2 * 2);
        let h = even(b[3].clamp(0.0, 1.0) * vh as f64).clamp(4, vh / 2 * 2);
        let x = even(b[0].clamp(0.0, 1.0) * vw as f64).min(vw - w);
        let y = even(b[1].clamp(0.0, 1.0) * vh as f64).min(vh - h);
        let (cur, next) = (format!("v{n}"), format!("v{}", n + 1));
        match style {
            RedactStyle::Solid => graph.push_str(&format!(";[{cur}]drawbox=x={x}:y={y}:w={w}:h={h}:color=black:t=fill{when}[{next}]")),
            _ => {
                let hide = if style == RedactStyle::Blur {
                    format!("gblur=sigma={}:steps=3", block * 2)
                } else {
                    format!("scale={}:{}:flags=area,scale={w}:{h}:flags=neighbor", w.div_ceil(block).max(1), h.div_ceil(block).max(1))
                };
                graph.push_str(&format!(
                    ";[{cur}]split=2[base{n}][part{n}];[part{n}]crop={w}:{h}:{x}:{y},{hide}[hid{n}];[base{n}][hid{n}]overlay={x}:{y}{when}[{next}]"
                ));
            }
        }
        n += 1;
    }
    Ff::new(env)
        .input(input)
        .arg("-filter_complex")
        .arg(graph)
        .args(["-map", &format!("[v{n}]"), "-map", "0:a:0?", "-map_metadata", "0"])
        .args(video_codec_args(env, in_fmt)?)
        .duration(p.duration)
        .run(out, cancel, progress)
}

/// Silence the given ranges (seconds), or cover them with a beep.
#[allow(clippy::too_many_arguments)]
pub fn bleep(env: &Env, input: &Path, in_fmt: Format, out: &Path, ranges: &[(f64, f64)], tone: bool, cancel: &Cancel, progress: &mut dyn FnMut(f64)) -> Result<()> {
    if ranges.is_empty() {
        return Err(Error::msg("Mark at least one part to bleep."));
    }
    let p = probe_opt(env, input);
    let rate = p.audio.as_ref().map(|a| a.sample_rate).unwrap_or(48_000);
    let layout = if p.audio.as_ref().is_some_and(|a| a.channels == 1) { "mono" } else { "stereo" };
    let any = ranges.iter().map(|(a, b)| format!("between(t,{a:.3},{b:.3})")).collect::<Vec<_>>().join("+");
    let graph = if tone {
        format!(
            "[0:a:0]volume=enable='{any}':volume=0[m];sine=f=1000:sample_rate={rate},aformat=channel_layouts={layout},volume=2.4,volume=enable='not({any})':volume=0[b];[m][b]amix=inputs=2:duration=first:normalize=0[a]"
        )
    } else {
        format!("[0:a:0]volume=enable='{any}':volume=0[a]")
    };
    Ff::new(env)
        .input(input)
        .arg("-filter_complex")
        .arg(graph)
        .args(["-map", "[a]", "-vn", "-map_metadata", "0"])
        .args(audio_codec_args(env, in_fmt, 192)?)
        .duration(p.duration)
        .run(out, cancel, progress)
}

/// `count` small frames spread through a video, `width` pixels wide, as PNGs in `dir`.
pub fn thumbnails(env: &Env, input: &Path, count: u32, width: u32, dir: &Path) -> Result<Vec<PathBuf>> {
    let d = probe(env, input)?.duration.filter(|d| *d > 0.0).ok_or_else(|| Error::msg("Could not read the video duration."))?;
    std::fs::create_dir_all(dir)?;
    let pattern = dir.join("thumb-%03d.png");
    Ff::new(env)
        .input(input)
        .arg("-vf")
        .arg(format!("fps={count}/{d:.3},scale={width}:-2"))
        .args(["-frames:v", &count.to_string(), "-an"])
        .run_pattern(&pattern, &dir.join("thumb-001.png"), &Cancel::new())?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("thumb-"))).collect();
    files.sort();
    Ok(files)
}

/// One frame at `at` seconds, `width` pixels wide, as a PNG (for previews).
pub fn frame_scaled(env: &Env, input: &Path, at: f64, width: u32, out: &Path) -> Result<()> {
    Ff::new(env)
        .arg("-ss")
        .arg(format!("{:.3}", at.max(0.0)))
        .input(input)
        .args(["-map", "0:v:0", "-frames:v", "1", "-update", "1", "-vf"])
        .arg(format!("scale={width}:-2"))
        .run(out, &Cancel::new(), &mut |_| {})
}

/// Loudness peaks for drawing a waveform: `count` values, 0–1.
pub fn peaks(env: &Env, input: &Path, count: usize) -> Result<Vec<f32>> {
    let mut cmd = env.ffmpeg_command().ok_or_else(|| Error::msg(FFMPEG_MISSING))?;
    cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-i"])
        .arg(input)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", "4000", "-f", "s16le", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::process::hide_console(&mut cmd);
    let output = cmd.output()?;
    let samples: Vec<i16> = output.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    if samples.is_empty() || count == 0 {
        return Err(Error::msg("This file has no sound to show."));
    }
    let per = (samples.len() as f64 / count as f64).max(1.0);
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let a = (k as f64 * per) as usize;
        let b = (((k + 1) as f64 * per) as usize).min(samples.len()).max(a + 1);
        let peak = samples[a.min(samples.len() - 1)..b].iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
        out.push(peak as f32 / 32768.0);
    }
    let top = out.iter().cloned().fold(0.0f32, f32::max).max(0.001);
    Ok(out.into_iter().map(|v| (v / top).sqrt()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atempo_stays_in_range() {
        assert_eq!(atempo_chain(2.0), "atempo=2");
        assert_eq!(atempo_chain(4.0), "atempo=2.0,atempo=2");
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.5");
    }
}
