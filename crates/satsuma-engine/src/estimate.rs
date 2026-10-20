//! Output size previews for the wheel: how big the result of a petal will be.
//!
//! Most operations simply run into a throwaway folder, so the size is exact. Long audio and
//! video would take too long, so a few seconds from the middle are converted and the size is
//! scaled to the full duration (an approximation).

use crate::plan::{plan, Job, Operation};
use crate::probe::probe;
use crate::run::{run_job, Context};
use crate::{Error, Result};
use satsuma_core::{Category, Format, Tool};
use std::path::{Path, PathBuf};

/// Media longer than this is sampled instead of converted in full.
const SAMPLE_ABOVE_SECS: f64 = 12.0;
const SAMPLE_SECS: f64 = 6.0;
/// Inputs up to this size are cheap enough to preview every petal ahead of time.
const CHEAP_BYTES: u64 = 25_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Estimate {
    /// The size the result will have.
    Exact(u64),
    /// Scaled up from a sample.
    About(u64),
}

impl Estimate {
    pub fn bytes(self) -> u64 {
        match self {
            Estimate::Exact(b) | Estimate::About(b) => b,
        }
    }

    /// "1.4 MB", or "≈ 12 MB" for sampled media.
    pub fn label(self) -> String {
        match self {
            Estimate::Exact(b) => satsuma_core::wheel::format_bytes(b),
            Estimate::About(b) => format!("≈ {}", satsuma_core::wheel::format_bytes(b)),
        }
    }
}

fn size_of(path: &Path) -> u64 {
    if path.is_dir() {
        std::fs::read_dir(path).map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum()).unwrap_or(0)
    } else {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

fn is_media(f: Format) -> bool {
    matches!(f.category(), Category::Audio | Category::Video)
}

/// Whether previewing every petal for these files in the background is cheap.
pub fn is_cheap(files: &[PathBuf], formats: &[Option<Format>]) -> bool {
    let bytes: u64 = files.iter().map(|f| size_of(f)).sum();
    bytes <= CHEAP_BYTES && formats.iter().all(|f| f.is_some_and(|f| !is_media(f)))
}

/// Runs `job` with its output redirected into `dir`; returns the result's size.
fn run_into(ctx: &Context, job: &Job, inputs: Vec<PathBuf>, dir: &Path) -> Result<u64> {
    let name = job.output.file_name().map(|n| n.to_owned()).unwrap_or_else(|| "output".into());
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&out_dir)?;
    let redirected = Job { inputs, output: out_dir.join(name), ..job.clone() };
    let produced = run_job(ctx, &redirected, &mut |_| {})?;
    Ok(size_of(&produced))
}

/// Operations whose output does not grow with the input's duration.
fn duration_independent(op: &Operation) -> bool {
    matches!(op.tool(), Some(Tool::VideoSnapshots))
}

fn estimate_job(ctx: &Context, job: &Job) -> Result<Estimate> {
    let dir = tempfile::Builder::new().prefix("satsuma-estimate-").tempdir()?;
    if job.inputs.len() == 1 && is_media(job.format) && !duration_independent(&job.op) {
        if let Some(total) = probe(ctx.env, &job.inputs[0]).ok().and_then(|p| p.duration) {
            if total > SAMPLE_ABOVE_SECS {
                if let Ok(e) = sampled(ctx, job, total, dir.path()) {
                    return Ok(e);
                }
            }
        }
    }
    Ok(Estimate::Exact(run_into(ctx, job, job.inputs.clone(), dir.path())?))
}

/// Converts a few seconds from the middle and scales the size to the whole duration.
fn sampled(ctx: &Context, job: &Job, total: f64, dir: &Path) -> Result<Estimate> {
    let input = &job.inputs[0];
    let ext = input.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let sample = dir.join(format!("sample.{ext}"));
    let start = (total / 2.0 - SAMPLE_SECS / 2.0).max(0.0);
    let mut cmd = ctx.env.ffmpeg_command().ok_or_else(|| Error::msg("ffmpeg is not installed."))?;
    cmd.args(["-loglevel", "error", "-ss"])
        .arg(format!("{start:.3}"))
        .arg("-i")
        .arg(input)
        .arg("-t")
        .arg(format!("{SAMPLE_SECS}"))
        .args(["-map", "0:v?", "-map", "0:a?", "-c", "copy"])
        .arg(&sample);
    crate::process::run(&mut cmd, ctx.cancel, &mut |_| {})?;
    let got = probe(ctx.env, &sample)?.duration.filter(|d| *d > 0.5).ok_or_else(|| Error::msg("Sample too short."))?;
    let bytes = run_into(ctx, job, vec![sample], dir)?;
    Ok(Estimate::About((bytes as f64 * total / got).round() as u64))
}

/// The size `op` on `files` will produce.
pub fn estimate(ctx: &Context, files: &[PathBuf], op: Operation) -> Result<Estimate> {
    let p = plan(ctx.env, files, op)?;
    if p.jobs.is_empty() {
        return Err(Error::msg(p.skipped.first().map(|s| s.1.clone()).unwrap_or_else(|| "Nothing to do.".into())));
    }
    let mut total = 0;
    let mut about = false;
    for job in &p.jobs {
        ctx.cancel.check()?;
        let e = estimate_job(ctx, job)?;
        about |= matches!(e, Estimate::About(_));
        total += e.bytes();
    }
    // Files already in the target format are skipped by the plan; count them as they are.
    total += p.skipped.iter().map(|(f, _)| size_of(f)).sum::<u64>();
    Ok(if about { Estimate::About(total) } else { Estimate::Exact(total) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cancel, Env};
    use satsuma_core::settings::Settings;

    #[test]
    fn exact_sizes_match_the_real_conversion() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("note.md");
        std::fs::write(&md, "# Title\n\nSome *text* here.\n").unwrap();
        let env = Env::discover();
        let settings = Settings::default();
        let cancel = Cancel::new();
        let ctx = Context { env: &env, settings: &settings, cancel: &cancel };
        let e = estimate(&ctx, std::slice::from_ref(&md), Operation::Convert(Format::Html)).unwrap();
        let real = crate::run_plan(&ctx, &plan(&env, std::slice::from_ref(&md), Operation::Convert(Format::Html)).unwrap(), &mut |_| {});
        assert_eq!(e, Estimate::Exact(size_of(&real.outputs[0])));
        // Nothing is left beside the source except the real conversion.
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(Estimate::About(1_500_000).label().starts_with("≈ 1.5"));
    }

    #[test]
    fn long_media_is_sampled() {
        let env = Env::discover();
        if !env.has_ffmpeg() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("long.wav");
        let mut cmd = env.ffmpeg_command().unwrap();
        cmd.args(["-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=40"]).arg(&wav);
        crate::process::run(&mut cmd, &Cancel::new(), &mut |_| {}).unwrap();
        let settings = Settings::default();
        let cancel = Cancel::new();
        let ctx = Context { env: &env, settings: &settings, cancel: &cancel };
        let e = estimate(&ctx, std::slice::from_ref(&wav), Operation::Convert(Format::Mp3)).unwrap();
        assert!(matches!(e, Estimate::About(_)), "{e:?}");
        let real = crate::run_plan(&ctx, &plan(&env, std::slice::from_ref(&wav), Operation::Convert(Format::Mp3)).unwrap(), &mut |_| {});
        let actual = size_of(&real.outputs[0]) as f64;
        let ratio = e.bytes() as f64 / actual;
        assert!((0.85..1.15).contains(&ratio), "estimate {} vs real {actual}", e.bytes());
    }
}
