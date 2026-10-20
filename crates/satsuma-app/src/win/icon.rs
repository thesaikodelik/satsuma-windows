//! Icons: the app artwork (a satsuma with a leaf on a teal tile, drawn like `icon.rs` on
//! macOS), the notification-area citrus slice, and HICON / .ico output.

use tiny_skia::{Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, RadialGradient, Rect, SpreadMode, Stroke, Transform};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

fn rgb(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, (a * 255.0).round() as u8)
}

fn oval(x: f32, y: f32, w: f32, h: f32) -> tiny_skia::Path {
    PathBuilder::from_oval(Rect::from_xywh(x, y, w, h).unwrap()).unwrap()
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish().unwrap()
}

fn solid(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

/// The app icon at `px` × `px`, straight RGBA. Drawn on a 1024-unit canvas.
pub fn app_icon(px: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(px, px).unwrap();
    let s = px as f32 / 1024.0;
    let t = Transform::from_scale(s, s);

    // Body: rounded square, teal gradient from top to bottom.
    let body = round_rect(100.0, 100.0, 824.0, 824.0, 185.0);
    let bg = LinearGradient::new(
        Point::from_xy(0.0, 100.0),
        Point::from_xy(0.0, 924.0),
        vec![GradientStop::new(0.0, rgb(22, 92, 94, 1.0)), GradientStop::new(1.0, rgb(9, 46, 52, 1.0))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pm.fill_path(&body, &Paint { shader: bg, anti_alias: true, ..Default::default() }, FillRule::Winding, t, None);

    // Shadow under the fruit.
    pm.fill_path(&oval(262.0, 738.0, 500.0, 90.0), &solid(rgb(0, 0, 0, 0.22)), FillRule::Winding, t, None);

    // Fruit: radial peel gradient lit from the upper left.
    let peel = RadialGradient::new(
        Point::from_xy(414.0, 442.0),
        0.0,
        Point::from_xy(414.0, 442.0),
        430.0,
        vec![GradientStop::new(0.0, rgb(255, 178, 64, 1.0)), GradientStop::new(1.0, rgb(244, 98, 18, 1.0))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pm.fill_path(&oval(232.0, 292.0, 560.0, 500.0), &Paint { shader: peel, anti_alias: true, ..Default::default() }, FillRule::Winding, t, None);

    // Dimples and highlight.
    for (x, y, r) in [(380.0, 560.0, 9.0), (460.0, 610.0, 7.0), (560.0, 600.0, 8.0), (640.0, 520.0, 7.0), (420.0, 440.0, 8.0), (600.0, 400.0, 9.0), (500.0, 330.0, 7.0), (340.0, 380.0, 7.0)] {
        let y = 1024.0 - y;
        pm.fill_path(&oval(x - r, y - r, r * 2.0, r * 2.0), &solid(rgb(255, 255, 255, 0.10)), FillRule::Winding, t, None);
    }
    pm.fill_path(&oval(330.0, 394.0, 120.0, 70.0), &solid(rgb(255, 255, 255, 0.35)), FillRule::Winding, t, None);

    // Stem.
    pm.fill_path(&round_rect(500.0, 262.0, 24.0, 50.0, 10.0), &solid(rgb(86, 60, 30, 1.0)), FillRule::Winding, t, None);

    // Leaf: designed with y up, translated to (520, 748) and turned 24°; flip to y down.
    let (sin, cos) = 24f32.to_radians().sin_cos();
    let map = |x: f32, y: f32| (520.0 + x * cos - y * sin, 1024.0 - (748.0 + x * sin + y * cos));
    let mut leaf = PathBuilder::new();
    let p0 = map(0.0, 0.0);
    leaf.move_to(p0.0, p0.1);
    let (a, b, c) = (map(60.0, 95.0), map(180.0, 95.0), map(230.0, 0.0));
    leaf.cubic_to(a.0, a.1, b.0, b.1, c.0, c.1);
    let (a, b, c) = (map(180.0, -80.0), map(60.0, -80.0), map(0.0, 0.0));
    leaf.cubic_to(a.0, a.1, b.0, b.1, c.0, c.1);
    leaf.close();
    let (g0, g1) = (map(115.0, -80.0), map(115.0, 95.0));
    let green = LinearGradient::new(
        Point::from_xy(g0.0, g0.1),
        Point::from_xy(g1.0, g1.1),
        vec![GradientStop::new(0.0, rgb(120, 200, 90, 1.0)), GradientStop::new(1.0, rgb(46, 140, 70, 1.0))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pm.fill_path(&leaf.finish().unwrap(), &Paint { shader: green, anti_alias: true, ..Default::default() }, FillRule::Winding, t, None);
    let mut vein = PathBuilder::new();
    let (v0, v1) = (map(10.0, 0.0), map(200.0, 0.0));
    vein.move_to(v0.0, v0.1);
    vein.line_to(v1.0, v1.1);
    let stroke = Stroke { width: 6.0, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
    pm.stroke_path(&vein.finish().unwrap(), &solid(rgb(255, 255, 255, 0.45)), &stroke, t, None);

    straight(&pm)
}

/// The notification-area citrus slice: a ring with six spokes, in `color`.
pub fn tray_icon(px: u32, white: bool) -> Vec<u8> {
    let mut pm = Pixmap::new(px, px).unwrap();
    let s = px as f32 / 18.0;
    let t = Transform::from_scale(s, s);
    let c = if white { Color::WHITE } else { Color::BLACK };
    let ring = oval(2.0, 2.0, 14.0, 14.0);
    pm.stroke_path(&ring, &solid(c), &Stroke { width: 1.6, ..Default::default() }, t, None);
    for k in 0..6 {
        let a = std::f32::consts::PI / 2.0 - k as f32 * std::f32::consts::PI / 3.0;
        let mut pb = PathBuilder::new();
        // y down: subtract the sine.
        pb.move_to(9.0 + 1.6 * a.cos(), 9.0 - 1.6 * a.sin());
        pb.line_to(9.0 + 5.6 * a.cos(), 9.0 - 5.6 * a.sin());
        let stroke = Stroke { width: 1.3, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
        pm.stroke_path(&pb.finish().unwrap(), &solid(c), &stroke, t, None);
    }
    straight(&pm)
}

fn straight(pm: &Pixmap) -> Vec<u8> {
    pm.pixels().iter().flat_map(|p| {
        let c = p.demultiply();
        [c.red(), c.green(), c.blue(), c.alpha()]
    }).collect()
}

/// An HICON from straight RGBA pixels.
pub fn hicon(rgba: &[u8], px: u32) -> Option<HICON> {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: px as i32,
                biHeight: -(px as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (px * px * 4) as usize);
        for (d, s) in dst.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
        }
        let mask = CreateBitmap(px as i32, px as i32, 1, 1, None);
        let ii = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&ii).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// A multi-size .ico (PNG-compressed entries) of the app icon.
pub fn write_ico(out: &std::path::Path) -> Result<(), String> {
    let sizes = [16u32, 20, 24, 32, 40, 48, 64, 128, 256];
    let mut images = Vec::new();
    for px in sizes {
        let rgba = app_icon(px);
        let mut png = Vec::new();
        image::RgbaImage::from_raw(px, px, rgba)
            .ok_or("icon buffer")?
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        images.push((px, png));
    }
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (px, png) in &images {
        let dim = if *px >= 256 { 0 } else { *px as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &images {
        ico.extend_from_slice(png);
    }
    std::fs::write(out, ico).map_err(|e| e.to_string())
}
