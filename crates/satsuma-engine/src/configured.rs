//! Running a tool with the choices made in its options window ([`ToolOptions`]).

use crate::plan::{Job, ToolOptions};
use crate::run::Context;
use crate::{image, imaging, media, pdf, Error, Result};
use satsuma_core::naming::stem_of;
use satsuma_core::settings::Strength;
use satsuma_core::{Category, Format, Tool};
use std::path::Path;

pub(crate) fn execute(ctx: &Context, job: &Job, tool: Tool, options: &ToolOptions, out: &Path, tmp: &Path, progress: &mut dyn FnMut(f64)) -> Result<()> {
    let env = ctx.env;
    let cancel = ctx.cancel;
    let input = &job.inputs[0];
    let fmt = job.format;
    let stem = stem_of(&input.file_name().unwrap_or_default().to_string_lossy()).to_string();
    match (tool, options) {
        (Tool::ChangeVideoSpeed, ToolOptions::Speed { factor, keep_pitch }) => {
            media::change_speed_with(env, input, fmt, out, *factor, *keep_pitch, cancel, progress)
        }
        (Tool::VideoSnapshots, ToolOptions::Snapshots { times, format }) => media::frames(env, input, times, *format, out, &stem, cancel, progress),
        (Tool::SplitVideo, ToolOptions::Split { points }) => media::split_at(env, input, fmt, out, &stem, points, cancel, progress).map(|_| ()),
        (Tool::Compress, ToolOptions::Compress { strength, target_bytes, max_edge }) => {
            compress(ctx, input, fmt, out, *strength, *target_bytes, *max_edge, tmp, progress)?;
            crate::run::ensure_smaller(input, out)
        }
        (Tool::AudioChannels, ToolOptions::Channels { stereo, left, right }) => {
            media::mix_channels(env, input, fmt, out, *stereo, *left, *right, cancel, progress)
        }
        (Tool::AudioToVideo, ToolOptions::Visualize { width, height, still }) => {
            media::visualize_with(env, input, out, *width, *height, *still, tmp, cancel, progress)
        }
        (Tool::CreateCollage, ToolOptions::Collage { layout, cell, gap, background }) => {
            image::collage_with(env, &job.inputs, out, tmp, *layout, *cell, *gap, *background, cancel, progress)
        }
        (Tool::RemoveMetadata, ToolOptions::Tags { clear, tags }) => match fmt.category() {
            _ if fmt == Format::Pdf => pdf::set_info(input, out, *clear, tags),
            Category::Audio | Category::Video => media::set_tags(env, input, fmt, out, *clear, tags, cancel, progress),
            _ => Err(Error::msg("Tags can be edited on audio, video and PDF files.")),
        },
        (Tool::TrimAudio | Tool::TrimVideo, ToolOptions::Trim { start, end }) => media::trim(env, input, fmt, out, *start, *end, cancel, progress),
        (Tool::CropVideo, ToolOptions::CropVideo { x, y, w, h }) => media::crop_video(env, input, fmt, out, (*x, *y, *w, *h), cancel, progress),
        (Tool::RedactAudio, ToolOptions::Bleep { ranges, tone }) => media::bleep(env, input, fmt, out, ranges, *tone, cancel, progress),
        (Tool::OrganizePdf, ToolOptions::Pages { pages }) => pdf::organize(input, out, pages),
        (Tool::AnnotateImage, ToolOptions::Annotate { svg }) => {
            let img = imaging::load(input)?;
            progress(0.3);
            let marked = imaging::overlay_svg(&img, svg)?;
            progress(0.7);
            imaging::write(&marked, out, Format::Png)
        }
        _ => Err(Error::msg(format!("{} can't use these options.", tool.info().name))),
    }
}

#[allow(clippy::too_many_arguments)]
fn compress(
    ctx: &Context,
    input: &Path,
    fmt: Format,
    out: &Path,
    strength: Strength,
    target: Option<u64>,
    max_edge: Option<u32>,
    tmp: &Path,
    progress: &mut dyn FnMut(f64),
) -> Result<()> {
    let (env, cancel) = (ctx.env, ctx.cancel);
    match (fmt.category(), target) {
        (Category::Image, Some(t)) => image::compress_to_size(env, input, fmt, out, t, max_edge, tmp, cancel),
        (Category::Image, None) => image::compress(env, input, fmt, out, strength, max_edge, tmp, cancel),
        (Category::Audio, Some(t)) => media::compress_audio_to(env, input, fmt, out, t, cancel, progress),
        (Category::Audio, None) => media::compress_audio(env, input, fmt, out, strength, cancel, progress),
        (Category::Video, Some(t)) => media::compress_video_to(env, input, fmt, out, t, cancel, progress),
        (Category::Video, None) => media::compress_video(env, input, fmt, out, strength, cancel, progress),
        _ if fmt == Format::Pdf => pdf::compress(input, out, strength == Strength::Strong),
        _ => Err(Error::msg("This file can't be compressed.")),
    }
}
