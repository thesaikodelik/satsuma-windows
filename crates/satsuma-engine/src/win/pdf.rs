//! PDF on Windows: lopdf for structure (merge, split, metadata, image recompression),
//! pdf-extract for text, and `Windows.Data.Pdf` (the system PDF renderer) for page images.
//! Mirrors the PDFKit/CoreGraphics version in `pdf.rs`.

use super::win::{ensure_com, winerr};
use crate::{Error, Result};
use image::RgbaImage;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use satsuma_core::Format;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

fn open(path: &Path) -> Result<Document> {
    let doc = Document::load(path).map_err(|e| {
        let text = e.to_string().to_ascii_lowercase();
        if text.contains("decrypt") || text.contains("password") || text.contains("encrypt") {
            Error::msg(format!("{} is password protected.", name(path)))
        } else {
            Error::msg(format!("Could not open the PDF {}.", name(path)))
        }
    })?;
    if doc.is_encrypted() {
        return Err(Error::msg(format!("{} is password protected.", name(path))));
    }
    Ok(doc)
}

fn save(doc: &mut Document, out: &Path) -> Result<()> {
    doc.save(out).map_err(|e| Error::msg(format!("Could not save the PDF: {e}")))?;
    Ok(())
}

pub fn page_count(path: &Path) -> Result<usize> {
    Ok(open(path)?.get_pages().len())
}

/// Selectable text of every page, pages separated by blank lines.
pub fn text(path: &Path) -> Result<String> {
    let doc = open(path)?;
    let bytes = std::fs::read(path)?;
    // Two extractors with different strengths: pdf-extract keeps paragraph breaks but can
    // split glyph runs ("1 1 9"); lopdf keeps words whole. Each page uses whichever reads
    // better. pdf-extract can panic on unusual files.
    let extracted = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes)).ok().and_then(|r| r.ok()).unwrap_or_default();
    let numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    let pages: Vec<String> = numbers
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let simple = doc.extract_text(&[*n]).unwrap_or_default();
            let rich = extracted.get(i).cloned().unwrap_or_default();
            if rich.trim().is_empty() || broken_tokens(&rich) > broken_tokens(&simple) {
                simple
            } else {
                rich
            }
        })
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    Ok(pages.join("\n\n"))
}

/// Share of single-character alphanumeric words, a sign of glyphs split apart.
fn broken_tokens(text: &str) -> f64 {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return 1.0;
    }
    let singles = words.iter().filter(|w| w.chars().count() == 1 && w.chars().all(char::is_alphanumeric)).count();
    singles as f64 / words.len() as f64
}

/// Merges whole documents in order (lopdf's documented merge procedure).
pub fn merge(inputs: &[PathBuf], out: &Path) -> Result<()> {
    if inputs.is_empty() {
        return Err(Error::msg("Select PDFs to merge."));
    }
    let mut max_id = 1;
    let mut pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut merged = Document::with_version("1.7");
    for input in inputs {
        let mut doc = open(input)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        for (_, id) in doc.get_pages() {
            pages.insert(id, doc.get_object(id).map_err(|_| Error::msg(format!("Could not read {}.", name(input))))?.to_owned());
        }
        objects.extend(doc.objects);
    }
    let mut catalog: Option<(ObjectId, Object)> = None;
    let mut root_pages: Option<(ObjectId, Object)> = None;
    for (id, object) in objects {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog.get_or_insert((id, object));
            }
            b"Pages" => {
                if let Ok(dict) = object.as_dict() {
                    let mut dict = dict.clone();
                    if let Some((_, ref old)) = root_pages {
                        if let Ok(old) = old.as_dict() {
                            dict.extend(old);
                        }
                    }
                    root_pages = Some((root_pages.as_ref().map(|r| r.0).unwrap_or(id), Object::Dictionary(dict)));
                }
            }
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                merged.objects.insert(id, object);
            }
        }
    }
    let (pages_id, pages_obj) = root_pages.ok_or_else(|| Error::msg("Could not merge the PDFs."))?;
    for (id, object) in &pages {
        if let Ok(dict) = object.as_dict() {
            let mut dict = dict.clone();
            dict.set("Parent", pages_id);
            merged.objects.insert(*id, Object::Dictionary(dict));
        }
    }
    let (catalog_id, catalog_obj) = catalog.ok_or_else(|| Error::msg("Could not merge the PDFs."))?;
    if let Ok(dict) = pages_obj.as_dict() {
        let mut dict = dict.clone();
        dict.set("Count", pages.len() as u32);
        dict.set("Kids", pages.keys().map(|id| Object::Reference(*id)).collect::<Vec<_>>());
        merged.objects.insert(pages_id, Object::Dictionary(dict));
    }
    if let Ok(dict) = catalog_obj.as_dict() {
        let mut dict = dict.clone();
        dict.set("Pages", pages_id);
        dict.remove(b"Outlines");
        merged.objects.insert(catalog_id, Object::Dictionary(dict));
    }
    merged.trailer.set("Root", catalog_id);
    merged.max_id = merged.objects.len() as u32;
    merged.renumber_objects();
    merged.compress();
    save(&mut merged, out)
}

/// One PDF per page in `out_dir`, named `"{stem} 01.pdf"`, `"{stem} 02.pdf"` …
pub fn split(input: &Path, out_dir: &Path, stem: &str, progress: &mut dyn FnMut(f64)) -> Result<Vec<PathBuf>> {
    let doc = open(input)?;
    let numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    let n = numbers.len();
    let width = n.to_string().len().max(2);
    std::fs::create_dir_all(out_dir)?;
    let mut outputs = Vec::new();
    for (i, keep) in numbers.iter().enumerate() {
        let mut page = doc.clone();
        let others: Vec<u32> = numbers.iter().copied().filter(|p| p != keep).collect();
        page.delete_pages(&others);
        page.prune_objects();
        page.renumber_objects();
        let out = out_dir.join(format!("{stem} {:0width$}.pdf", i + 1));
        save(&mut page, &out)?;
        outputs.push(out);
        progress((i + 1) as f64 / n.max(1) as f64);
    }
    Ok(outputs)
}

/// Removes the document information dictionary and XMP metadata.
pub fn strip_metadata(input: &Path, out: &Path) -> Result<()> {
    let mut doc = open(input)?;
    doc.trailer.remove(b"Info");
    if let Ok(catalog) = doc.catalog_mut() {
        catalog.remove(b"Metadata");
    }
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        if let Some(Object::Stream(s)) = doc.objects.get(&id) {
            if s.dict.get(b"Type").and_then(|t| t.as_name()).ok() == Some(b"Metadata".as_slice()) {
                doc.objects.remove(&id);
            }
        } else if let Some(Object::Dictionary(d)) = doc.objects.get_mut(&id) {
            d.remove(b"Metadata");
        }
    }
    doc.prune_objects();
    save(&mut doc, out)
}

/// Number of colour components of an image colour space we can re-encode, if any.
fn components(doc: &Document, cs: Option<&Object>) -> Option<u8> {
    let cs = cs?;
    let cs = match cs {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    match cs {
        Object::Name(n) if n == b"DeviceRGB" => Some(3),
        Object::Name(n) if n == b"DeviceGray" => Some(1),
        Object::Array(a) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased".as_slice()) => {
            let id = a.get(1)?.as_reference().ok()?;
            let n = doc.get_object(id).ok()?.as_stream().ok()?.dict.get(b"N").ok()?.as_i64().ok()?;
            matches!(n, 1 | 3).then_some(n as u8)
        }
        _ => None,
    }
}

/// Re-encodes embedded images as JPEG (and downsamples them for the screen when `strong`),
/// like PDFKit's "save images as JPEG" and "optimize for screen" options.
pub fn compress(input: &Path, out: &Path, strong: bool) -> Result<()> {
    let mut doc = open(input)?;
    let quality = if strong { 50 } else { 70 };
    let max_edge = if strong { 1600 } else { 3000 };
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        let Some(Object::Stream(stream)) = doc.objects.get(&id) else { continue };
        let dict = &stream.dict;
        if dict.get(b"Subtype").and_then(|s| s.as_name()).ok() != Some(b"Image".as_slice()) {
            continue;
        }
        if dict.get(b"BitsPerComponent").and_then(|b| b.as_i64()).unwrap_or(8) != 8 || dict.has(b"Decode") || dict.has(b"Mask") {
            continue;
        }
        let filter = dict.get(b"Filter").ok().and_then(|f| match f {
            Object::Name(n) => Some(n.clone()),
            Object::Array(a) if a.len() == 1 => a[0].as_name().ok().map(|n| n.to_vec()),
            _ => None,
        });
        let (w, h) = (
            dict.get(b"Width").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
            dict.get(b"Height").and_then(|v| v.as_i64()).unwrap_or(0) as u32,
        );
        if w == 0 || h == 0 {
            continue;
        }
        let decoded: Option<image::DynamicImage> = match filter.as_deref() {
            Some(b"DCTDecode") => image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg).ok(),
            Some(b"FlateDecode") | None => {
                let Some(n) = components(&doc, dict.get(b"ColorSpace").ok()) else { continue };
                let Ok(raw) = stream.decompressed_content() else { continue };
                match n {
                    3 => image::RgbImage::from_raw(w, h, raw).map(image::DynamicImage::ImageRgb8),
                    _ => image::GrayImage::from_raw(w, h, raw).map(image::DynamicImage::ImageLuma8),
                }
            }
            _ => None,
        };
        let Some(mut img) = decoded else { continue };
        if img.width().max(img.height()) > max_edge {
            img = img.resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3);
        }
        let gray = img.color().channel_count() == 1;
        let mut jpeg = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality);
        let ok = if gray { enc.encode_image(&img.to_luma8()) } else { enc.encode_image(&img.to_rgb8()) };
        if ok.is_err() || jpeg.len() >= stream.content.len() {
            continue;
        }
        let mut new_dict = dict.clone();
        new_dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
        new_dict.remove(b"DecodeParms");
        new_dict.set("Width", img.width() as i64);
        new_dict.set("Height", img.height() as i64);
        new_dict.set("ColorSpace", Object::Name(if gray { b"DeviceGray".to_vec() } else { b"DeviceRGB".to_vec() }));
        new_dict.set("BitsPerComponent", 8);
        let mut s = Stream::new(new_dict, jpeg);
        s.allows_compression = false;
        doc.objects.insert(id, Object::Stream(s));
    }
    doc.trailer.remove(b"Info");
    doc.prune_objects();
    doc.compress();
    save(&mut doc, out)
}

fn absolute(path: &Path) -> PathBuf {
    let p = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    // StorageFile does not accept `\\?\` verbatim paths.
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => p,
    }
}

/// Renders each page to PNG or JPG at `dpi`, writing to `name_for(index, count)`.
pub fn render_pages(
    input: &Path,
    format: Format,
    dpi: f64,
    name_for: &dyn Fn(usize, usize) -> PathBuf,
    progress: &mut dyn FnMut(f64),
) -> Result<Vec<PathBuf>> {
    use windows::core::HSTRING;
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::BitmapEncoder;
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};

    if !matches!(format, Format::Png | Format::Jpg) {
        return Err(Error::msg(format!("Cannot render PDF pages as {}.", format.label())));
    }
    ensure_com();
    let unreadable = |e| winerr(&format!("Could not render {}", name(input)), e);
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(absolute(input).as_os_str()))
        .and_then(|op| op.join())
        .map_err(unreadable)?;
    let doc = PdfDocument::LoadFromFileAsync(&file).and_then(|op| op.join()).map_err(|e| {
        if doc_is_locked(input) {
            Error::msg(format!("{} is password protected.", name(input)))
        } else {
            unreadable(e)
        }
    })?;
    let count = doc.PageCount().map_err(unreadable)? as usize;
    let mut outputs = Vec::with_capacity(count);
    for i in 0..count {
        let page = doc.GetPage(i as u32).map_err(unreadable)?;
        // Page sizes are in DIPs (1/96 inch), after the crop box and rotation are applied.
        let size = page.Size().map_err(unreadable)?;
        let (wi, hi) = (size.Width as f64 / 96.0, size.Height as f64 / 96.0);
        let scale = dpi.min(16000.0 / (wi.max(hi) * 1.0).max(1e-3));
        let (pw, ph) = (((wi * scale).round() as u32).max(1), ((hi * scale).round() as u32).max(1));
        let opts = PdfPageRenderOptions::new().map_err(unreadable)?;
        opts.SetDestinationWidth(pw).map_err(unreadable)?;
        opts.SetDestinationHeight(ph).map_err(unreadable)?;
        opts.SetBackgroundColor(windows::UI::Color { A: 255, R: 255, G: 255, B: 255 }).map_err(unreadable)?;
        opts.SetBitmapEncoderId(BitmapEncoder::PngEncoderId().map_err(unreadable)?).map_err(unreadable)?;
        let stream = InMemoryRandomAccessStream::new().map_err(unreadable)?;
        page.RenderWithOptionsToStreamAsync(&stream, &opts).and_then(|op| op.join()).map_err(unreadable)?;
        let len = stream.Size().map_err(unreadable)? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(unreadable)?).map_err(unreadable)?;
        reader.LoadAsync(len).and_then(|op| op.join()).map_err(unreadable)?;
        let mut png = vec![0u8; len as usize];
        reader.ReadBytes(&mut png).map_err(unreadable)?;

        let out = name_for(i, count);
        match format {
            Format::Png => write_png_with_dpi(&png, &out, dpi)?,
            _ => {
                let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
                    .map_err(|_| Error::msg("Could not render the PDF page."))?
                    .to_rgb8();
                let mut file = std::io::BufWriter::new(std::fs::File::create(&out)?);
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 90);
                enc.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(dpi.round() as u16));
                enc.encode_image(&img).map_err(|e| Error::msg(format!("Could not save the page: {e}")))?;
            }
        }
        outputs.push(out);
        progress((i + 1) as f64 / count.max(1) as f64);
    }
    Ok(outputs)
}

fn doc_is_locked(path: &Path) -> bool {
    matches!(Document::load(path), Ok(d) if d.is_encrypted())
}

/// Writes PNG bytes with a pHYs chunk recording `dpi` (inserted after IHDR).
fn write_png_with_dpi(png: &[u8], out: &Path, dpi: f64) -> Result<()> {
    const SIG: usize = 8;
    if png.len() < SIG + 25 || &png[12..16] != b"IHDR" {
        std::fs::write(out, png)?;
        return Ok(());
    }
    let ihdr_end = SIG + 8 + 13 + 4;
    let ppm = (dpi / 0.0254).round() as u32;
    let mut chunk = Vec::with_capacity(21);
    chunk.extend_from_slice(&9u32.to_be_bytes());
    let mut body = b"pHYs".to_vec();
    body.extend_from_slice(&ppm.to_be_bytes());
    body.extend_from_slice(&ppm.to_be_bytes());
    body.push(1);
    let crc = crc32(&body);
    chunk.extend_from_slice(&body);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut data = Vec::with_capacity(png.len() + chunk.len());
    data.extend_from_slice(&png[..ihdr_end]);
    data.extend_from_slice(&chunk);
    data.extend_from_slice(&png[ihdr_end..]);
    std::fs::write(out, data)?;
    Ok(())
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Whether an image looks photographic (many distinct colours), so JPEG suits it.
fn is_photographic(img: &RgbaImage) -> bool {
    let mut seen = std::collections::HashSet::new();
    let step = ((img.width() as usize * img.height() as usize) / 20_000).max(1);
    for p in img.pixels().step_by(step) {
        seen.insert([p.0[0] >> 2, p.0[1] >> 2, p.0[2] >> 2]);
        if seen.len() > 2048 {
            return true;
        }
    }
    false
}

/// A one-page PDF holding `img` at 72 dpi (what `sips -s format pdf` writes).
pub fn image_pdf(img: &RgbaImage, out: &Path) -> Result<()> {
    let (w, h) = img.dimensions();
    let mut doc = Document::with_version("1.7");
    let alpha: Vec<u8> = img.pixels().map(|p| p.0[3]).collect();
    let has_alpha = alpha.iter().any(|&a| a < 255);
    let rgb = image::RgbImage::from_fn(w, h, |x, y| {
        let p = img.get_pixel(x, y).0;
        image::Rgb([p[0], p[1], p[2]])
    });

    let mut xobject = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => w as i64,
        "Height" => h as i64,
        "ColorSpace" => "DeviceRGB",
        "BitsPerComponent" => 8,
    };
    if has_alpha {
        let mut smask = Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64,
                "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8,
            },
            alpha,
        );
        let _ = smask.compress();
        let smask_id = doc.add_object(smask);
        xobject.set("SMask", smask_id);
    }
    let image_stream = if is_photographic(img) {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 92)
            .encode_image(&rgb)
            .map_err(|e| Error::msg(format!("Could not save the PDF: {e}")))?;
        xobject.set("Filter", "DCTDecode");
        let mut s = Stream::new(xobject, jpeg);
        s.allows_compression = false;
        s
    } else {
        let mut s = Stream::new(xobject, rgb.into_raw());
        let _ = s.compress();
        s
    };
    let image_id = doc.add_object(image_stream);
    let content = format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q");
    let mut content = Stream::new(Dictionary::new(), content.into_bytes());
    let _ = content.compress();
    let content_id = doc.add_object(content);
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), (w as i64).into(), (h as i64).into()],
        "Contents" => content_id,
        "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    save(&mut doc, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pdf(dir: &Path, name: &str, color: [u8; 4]) -> PathBuf {
        let p = dir.join(name);
        image_pdf(&RgbaImage::from_pixel(200, 100, image::Rgba(color)), &p).unwrap();
        p
    }

    #[test]
    fn merge_split_and_strip() {
        let dir = tempfile::tempdir().unwrap();
        let a = sample_pdf(dir.path(), "a.pdf", [255, 0, 0, 255]);
        let b = sample_pdf(dir.path(), "b.pdf", [0, 0, 255, 255]);
        assert_eq!(page_count(&a).unwrap(), 1);
        let merged = dir.path().join("m.pdf");
        merge(&[a.clone(), b.clone(), a], &merged).unwrap();
        assert_eq!(page_count(&merged).unwrap(), 3);
        let parts = split(&merged, &dir.path().join("parts"), "m", &mut |_| {}).unwrap();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].ends_with("m 01.pdf"));
        for p in &parts {
            assert_eq!(page_count(p).unwrap(), 1);
        }
        let clean = dir.path().join("clean.pdf");
        strip_metadata(&merged, &clean).unwrap();
        assert_eq!(page_count(&clean).unwrap(), 3);
    }

    #[test]
    fn renders_pages_with_the_system_renderer() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = sample_pdf(dir.path(), "r.pdf", [0, 128, 0, 255]);
        let out = render_pages(&pdf, Format::Png, 144.0, &|_, _| dir.path().join("r.png"), &mut |_| {}).unwrap();
        let img = image::open(&out[0]).unwrap().to_rgb8();
        // 200 × 100 pt at 144 dpi.
        assert_eq!(img.dimensions(), (400, 200));
        let px = img.get_pixel(200, 100).0;
        assert!(px[1] > 100 && px[0] < 40, "{px:?}");
    }
}
