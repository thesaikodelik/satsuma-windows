//! What this computer can actually do, so the wheel only shows working petals.

use crate::env::Env;
use crate::{image, media};
use satsuma_core::Format::{self, *};
use satsuma_core::{Category, Tool, WheelItem};

pub fn conversion_available(env: &Env, from: Format, to: Format) -> bool {
    if matches!(from, Pages | Ppt | Pptx) || matches!(to, Pages | Pptx) {
        return false;
    }
    if from == Pdf && to == Xlsx {
        return false;
    }
    match from.category() {
        Category::Image => {
            if to == Docx {
                true // Vision text recognition, or the picture itself
            } else if to == Webp {
                env.has_encoder("libwebp")
            } else {
                image::sips_format(to).is_some() && can_write_image(to)
            }
        }
        Category::Audio => env.has_ffmpeg() && media::audio_codec_args(env, to, 192).is_ok(),
        Category::Video => {
            env.has_ffmpeg()
                && match to {
                    Gif => true,
                    Mp3 => env.has_encoder("libmp3lame"),
                    _ => media::video_codec_args(env, to).is_ok(),
                }
        }
        Category::Document | Category::Spreadsheet => !matches!(to, Pdf | Png | Jpg) || can_print(env),
        Category::Subtitle | Category::Archive => true,
    }
}

/// Image formats this machine can encode (on Windows, HEIC needs the Store's HEVC extension
/// and AVIF needs an AV1 encoder in ffmpeg).
#[cfg(windows)]
fn can_write_image(f: Format) -> bool {
    crate::win::raster::can_write(f)
}

#[cfg(not(windows))]
fn can_write_image(_: Format) -> bool {
    true
}

/// Documents become PDFs (and page images) through CoreText on macOS, Edge on Windows.
#[cfg(windows)]
fn can_print(env: &Env) -> bool {
    env.browser.is_some()
}

#[cfg(not(windows))]
fn can_print(_: &Env) -> bool {
    true
}

fn has_h264(env: &Env) -> bool {
    env.pick_encoder(&["libx264", "h264_videotoolbox", "h264_mf"]).is_some()
}

pub fn tool_available(env: &Env, tool: Tool, input: Format) -> bool {
    use Tool::*;
    let ff = env.has_ffmpeg();
    let audio_codec = |f: Format| ff && media::audio_codec_args(env, f, 192).is_ok();
    if tool.is_interactive() {
        if !tool.has_editor() {
            return false;
        }
        return match tool {
            TrimVideo | CropVideo | RedactVideo => {
                ff && input.category() == Category::Video && input != Gif && media::video_codec_args(env, input).is_ok()
            }
            RedactAudio => audio_codec(input),
            OrganizePdf => input == Pdf,
            _ => (matches!(input, Jpg | Png | Tiff) || (input == Heic && can_write_image(Heic))) || (input == Webp && env.has_encoder("libwebp")),
        };
    }
    match tool {
        Compress => match input {
            Jpg | Tiff => true,
            Heic | Avif => can_write_image(input),
            Png => ff,
            Webp => env.has_encoder("libwebp"),
            Pdf => true,
            Wav | Flac => ff,
            Mp3 | M4a => audio_codec(input),
            Mp4 | Mov | Mkv => ff && (has_h264(env) || env.pick_encoder(&["hevc_videotoolbox", "libx265"]).is_some()),
            _ => false,
        },
        RemoveMetadata => match input.category() {
            Category::Image => matches!(input, Jpg | Png | Webp),
            Category::Audio => ff,
            Category::Video => input == Gif || ff,
            _ => input == Pdf,
        },
        NormalizeAudio | TrimAudio | AudioChannels => audio_codec(input),
        AudioToVideo => ff && has_h264(env) && media::audio_codec_args(env, Mp4, 192).is_ok(),
        MuteVideo | VideoSnapshots => ff,
        ChangeVideoSpeed | SplitVideo => ff && media::video_codec_args(env, input).is_ok(),
        JoinVideos => ff && has_h264(env),
        CreateCollage => ff,
        SplitPdf | MergePdf | CreatePdf | ExtractArchive => true,
        _ => false,
    }
}

/// Whether `item` works for every file in the selection (`None` = folder or unknown file).
pub fn item_available(env: &Env, item: &WheelItem, formats: &[Option<Format>]) -> bool {
    match item {
        WheelItem::Action(_) => true,
        WheelItem::Convert(to) => formats
            .iter()
            .all(|f| f.is_some_and(|f| f == *to || conversion_available(env, f, *to))),
        WheelItem::Tool(t) => formats.iter().all(|f| f.is_some_and(|f| tool_available(env, *t, f))),
    }
}

/// Counts the conversion paths available on this machine.
pub fn available_paths(env: &Env) -> usize {
    Format::ALL
        .iter()
        .map(|f| {
            satsuma_core::matrix::conversion_targets(*f)
                .into_iter()
                .filter(|t| conversion_available(env, *f, *t))
                .count()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_ffmpeg_only_system_backends_remain() {
        let env = Env::without_ffmpeg();
        assert!(conversion_available(&env, Png, Jpg));
        assert!(!conversion_available(&env, Png, Webp));
        assert!(!conversion_available(&env, Wav, Mp3));
        assert!(conversion_available(&env, Docx, Pdf));
        assert!(conversion_available(&env, Csv, Xlsx));
        assert!(!conversion_available(&env, Docx, Pages));
        assert!(!conversion_available(&env, Pdf, Xlsx));
        assert!(tool_available(&env, Tool::MergePdf, Pdf));
        assert!(!tool_available(&env, Tool::NormalizeAudio, Mp3));
        assert!(!tool_available(&env, Tool::AnnotateImage, Jpg));
        assert!(tool_available(&env, Tool::EditImage, Jpg));
        assert!(tool_available(&env, Tool::CropImage, Jpg));
        assert!(tool_available(&env, Tool::FrameImage, Heic));
        assert!(!tool_available(&env, Tool::CropImage, Webp));
        assert!(conversion_available(&env, Jpg, Docx));
        assert!(tool_available(&env, Tool::RemoveMetadata, Jpg));
        assert!(!tool_available(&env, Tool::RemoveMetadata, Heic));
    }

    #[test]
    fn folders_only_get_actions() {
        let env = Env::without_ffmpeg();
        assert!(!item_available(&env, &WheelItem::Convert(Jpg), &[None]));
        assert!(item_available(&env, &WheelItem::Action(satsuma_core::Action::Copy), &[None]));
    }
}
