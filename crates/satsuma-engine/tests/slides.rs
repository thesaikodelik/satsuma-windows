//! PDF → PowerPoint and Excel, and slides → PDF through PowerPoint when it's installed.

use satsuma_core::settings::Settings;
use satsuma_core::Format;
use satsuma_engine::{plan, run_job, Cancel, Context, Env, Operation};
use std::path::{Path, PathBuf};

fn convert(env: &Env, input: &Path, to: Format) -> PathBuf {
    let settings = Settings::default();
    let cancel = Cancel::new();
    let ctx = Context { env, settings: &settings, cancel: &cancel };
    let p = plan(env, &[input.to_path_buf()], Operation::Convert(to)).expect("plan");
    assert!(!p.jobs.is_empty(), "{:?}", p.skipped);
    run_job(&ctx, &p.jobs[0], &mut |_| {}).unwrap_or_else(|e| panic!("{to:?}: {e}"))
}

/// A two-page PDF: a table, then a plain page.
fn table_pdf(env: &Env, dir: &Path) -> PathBuf {
    let pages = table_pages(dir);
    let pdf = dir.join("prices.pdf");
    satsuma_engine::image::images_to_pdf(env, &pages, &pdf, dir, &Cancel::new(), &mut |_| {}).unwrap();
    pdf
}

/// Pages drawn as SVG (with text), rendered to PNG through the engine.
fn table_pages(dir: &Path) -> Vec<PathBuf> {
    let rows = [("Item", "Price", "Qty"), ("Green tea", "4.50", "2"), ("Coffee", "3.25", "10"), ("Orange juice", "5.00", "1")];
    let mut text = String::new();
    for (i, (a, b, c)) in rows.iter().enumerate() {
        let y = 160 + i * 90;
        text.push_str(&format!(
            r#"<text x="100" y="{y}" font-size="44">{a}</text><text x="700" y="{y}" font-size="44">{b}</text><text x="1000" y="{y}" font-size="44">{c}</text>"#
        ));
    }
    let mut pages = Vec::new();
    for (n, body) in [text, r#"<text x="100" y="200" font-size="60">Thank you</text>"#.to_string()].iter().enumerate() {
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1275" height="1650"><rect width="1275" height="1650" fill="white"/><g font-family="Segoe UI">{body}</g></svg>"#);
        let img = satsuma_engine::imaging::render_svg(&svg, 1275, 1650).unwrap();
        let p = dir.join(format!("page{n}.png"));
        img.save(&p).unwrap();
        pages.push(p);
    }
    pages
}

#[test]
fn pdf_to_pptx_and_xlsx() {
    let env = Env::discover();
    let dir = tempfile::tempdir().unwrap();
    let pdf = table_pdf(&env, dir.path());

    let deck = convert(&env, &pdf, Format::Pptx);
    let zip = zip::ZipArchive::new(std::fs::File::open(&deck).unwrap()).unwrap();
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok()).map(|n| n.into_owned()).collect();
    assert!(names.iter().any(|n| n == "ppt/slides/slide2.xml") && !names.iter().any(|n| n == "ppt/slides/slide3.xml"), "{names:?}");

    let book = convert(&env, &pdf, Format::Xlsx);
    let sheet = satsuma_engine::sheet::read(&book, Format::Xlsx).unwrap();
    let cells: Vec<Vec<String>> = sheet.rows.iter().map(|r| r.iter().map(|c| c.display()).collect()).collect();
    assert!(cells.iter().any(|r| r.len() >= 3 && r[0] == "Green tea" && r[1] == "4.5" && r[2] == "2"), "{cells:?}");
    assert!(cells.iter().any(|r| r.first().is_some_and(|c| c == "Thank you")), "{cells:?}");

    // Back again through PowerPoint, when it's here: the deck must open and keep both slides.
    if satsuma_engine::win::office::can_convert_slides() {
        let back = convert(&env, &deck, Format::Pdf);
        assert_eq!(satsuma_engine::pdf::page_count(&back).unwrap(), 2);
    } else {
        eprintln!("no PowerPoint or LibreOffice; skipping slides → PDF");
    }
}
