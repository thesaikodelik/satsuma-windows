//! A minimal PowerPoint writer: one picture per slide, filling it (PDF → PowerPoint).

use crate::{Error, Result};
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;

const NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const EMPTY_TREE: &str = r#"<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree>"#;

/// Slides are 13.333 in wide (PowerPoint's widescreen width); the height follows the pages.
const SLIDE_W: u64 = 12_192_000;

fn theme() -> String {
    let colours = [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "1F1F1F"),
        ("lt2", "EEEEEE"),
        ("accent1", "FF7A1A"),
        ("accent2", "4472C4"),
        ("accent3", "A5A5A5"),
        ("accent4", "FFC000"),
        ("accent5", "5B9BD5"),
        ("accent6", "70AD47"),
        ("hlink", "0563C1"),
        ("folHlink", "954F72"),
    ];
    let scheme: String = colours.iter().map(|(n, c)| format!(r#"<a:{n}><a:srgbClr val="{c}"/></a:{n}>"#)).collect();
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let line = r#"<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>"#;
    let effect = "<a:effectStyle><a:effectLst/></a:effectStyle>";
    format!(
        r#"{HEAD}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Satsuma"><a:themeElements><a:clrScheme name="Satsuma">{scheme}</a:clrScheme><a:fontScheme name="Satsuma"><a:majorFont><a:latin typeface="Segoe UI"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Segoe UI"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Satsuma"><a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst><a:lnStyleLst>{line}{line}{line}</a:lnStyleLst><a:effectStyleLst>{effect}{effect}{effect}</a:effectStyleLst><a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#
    )
}

fn rels(items: &[(&str, &str, String)]) -> String {
    let body: String = items.iter().map(|(id, kind, target)| format!(r#"<Relationship Id="{id}" Type="{REL}/{kind}" Target="{target}"/>"#)).collect();
    format!(r#"{HEAD}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{body}</Relationships>"#)
}

/// Writes a presentation with one JPEG per slide; `pages` are (JPEG bytes, width, height).
pub fn write_pictures(out: &Path, pages: &[(Vec<u8>, u32, u32)]) -> Result<()> {
    let Some((_, w0, h0)) = pages.first() else {
        return Err(Error::msg("There are no pages to put on slides."));
    };
    // PowerPoint accepts slide sides of 1 to 56 inches; very tall or wide pages are letterboxed.
    let slide_h = ((SLIDE_W as f64 * *h0 as f64 / *w0 as f64).round() as u64).clamp(914_400, 51_206_400);
    let z = |e: zip::result::ZipError| Error::msg(format!("Could not write the presentation: {e}"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(out)?);
    let opts = SimpleFileOptions::default();
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let put = |zip: &mut zip::ZipWriter<std::fs::File>, name: &str, data: &[u8], o: SimpleFileOptions| -> Result<()> {
        zip.start_file(name, o).map_err(z)?;
        zip.write_all(data)?;
        Ok(())
    };

    let slide_types: String = (1..=pages.len())
        .map(|i| format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#))
        .collect();
    let types = format!(
        r#"{HEAD}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>{slide_types}</Types>"#
    );
    put(&mut zip, "[Content_Types].xml", types.as_bytes(), opts)?;
    put(&mut zip, "_rels/.rels", rels(&[("rId1", "officeDocument", "ppt/presentation.xml".into())]).as_bytes(), opts)?;

    let ids: String = (1..=pages.len()).map(|i| format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, i + 2)).collect();
    let presentation = format!(
        r#"{HEAD}<p:presentation {NS} saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{SLIDE_W}" cy="{slide_h}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
    );
    put(&mut zip, "ppt/presentation.xml", presentation.as_bytes(), opts)?;
    let mut prels = vec![("rId1", "slideMaster", "slideMasters/slideMaster1.xml".to_string()), ("rId2", "theme", "theme/theme1.xml".to_string())];
    let slide_ids: Vec<String> = (1..=pages.len()).map(|i| format!("rId{}", i + 2)).collect();
    for (i, id) in slide_ids.iter().enumerate() {
        prels.push((id.as_str(), "slide", format!("slides/slide{}.xml", i + 1)));
    }
    put(&mut zip, "ppt/_rels/presentation.xml.rels", rels(&prels).as_bytes(), opts)?;

    let master = format!(
        r#"{HEAD}<p:sldMaster {NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FFFFFF"/></a:solidFill><a:effectLst/></p:bgPr></p:bg>{EMPTY_TREE}</p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
    );
    put(&mut zip, "ppt/slideMasters/slideMaster1.xml", master.as_bytes(), opts)?;
    put(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml".into()), ("rId2", "theme", "../theme/theme1.xml".into())]).as_bytes(),
        opts,
    )?;
    let layout = format!(r#"{HEAD}<p:sldLayout {NS} type="blank" preserve="1"><p:cSld name="Blank">{EMPTY_TREE}</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#);
    put(&mut zip, "ppt/slideLayouts/slideLayout1.xml", layout.as_bytes(), opts)?;
    put(&mut zip, "ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(&[("rId1", "slideMaster", "../slideMasters/slideMaster1.xml".into())]).as_bytes(), opts)?;
    put(&mut zip, "ppt/theme/theme1.xml", theme().as_bytes(), opts)?;

    for (i, (jpeg, w, h)) in pages.iter().enumerate() {
        let n = i + 1;
        // Fit the page inside the slide, centred (pages of other sizes keep their shape).
        let k = (SLIDE_W as f64 / *w as f64).min(slide_h as f64 / *h as f64);
        let (cx, cy) = ((*w as f64 * k).round() as u64, (*h as f64 * k).round() as u64);
        let (x, y) = ((SLIDE_W - cx.min(SLIDE_W)) / 2, (slide_h - cy.min(slide_h)) / 2);
        let slide = format!(
            r#"{HEAD}<p:sld {NS}><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:pic><p:nvPicPr><p:cNvPr id="2" name="Page {n}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
        );
        put(&mut zip, &format!("ppt/slides/slide{n}.xml"), slide.as_bytes(), opts)?;
        put(
            &mut zip,
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            rels(&[("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml".into()), ("rId2", "image", format!("../media/image{n}.jpeg"))]).as_bytes(),
            opts,
        )?;
        put(&mut zip, &format!("ppt/media/image{n}.jpeg"), jpeg, stored)?;
    }
    zip.finish().map_err(z)?;
    Ok(())
}
