//! Edit Photo on Windows: the CoreImage filter chain of `photo.rs`, reimplemented on float
//! pixels with the same parameter mapping (exposure, tone curve, colour controls, gamma,
//! temperature and tint, vibrance, clarity/texture, noise reduction, sharpness, vignette,
//! grain).

use crate::imaging::{self, Image};
use crate::Result;
use image::RgbaImage;
use satsuma_core::edit::Adjustments;
use std::sync::Arc;

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn luma(p: [f32; 3]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}

/// Planar float image in sRGB-encoded 0–1 values.
struct Planes {
    w: usize,
    h: usize,
    rgb: Vec<[f32; 3]>,
    alpha: Vec<u8>,
}

impl Planes {
    fn from(img: &RgbaImage) -> Planes {
        let rgb = img.pixels().map(|p| [p.0[0] as f32 / 255.0, p.0[1] as f32 / 255.0, p.0[2] as f32 / 255.0]).collect();
        let alpha = img.pixels().map(|p| p.0[3]).collect();
        Planes { w: img.width() as usize, h: img.height() as usize, rgb, alpha }
    }

    fn into_image(self) -> RgbaImage {
        let mut out = RgbaImage::new(self.w as u32, self.h as u32);
        for ((dst, c), a) in out.pixels_mut().zip(self.rgb).zip(self.alpha) {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            dst.0 = [q(c[0]), q(c[1]), q(c[2]), a];
        }
        out
    }

    fn map(&mut self, f: impl Fn([f32; 3]) -> [f32; 3]) {
        for p in &mut self.rgb {
            *p = f(*p);
        }
    }

    /// Gaussian-like blur (three box passes) of a single channel extracted with `get`.
    fn blurred(&self, sigma: f32, get: impl Fn([f32; 3]) -> [f32; 3]) -> Vec<[f32; 3]> {
        let mut data: Vec<[f32; 3]> = self.rgb.iter().map(|p| get(*p)).collect();
        if sigma < 0.5 {
            return data;
        }
        let ideal = ((12.0 * sigma * sigma / 3.0) + 1.0).sqrt();
        let r = (((ideal - 1.0) / 2.0).floor().max(0.0)) as usize;
        let mut tmp = data.clone();
        for _ in 0..3 {
            box_pass(&data, &mut tmp, self.w, self.h, r, true);
            box_pass(&tmp, &mut data, self.w, self.h, r, false);
        }
        data
    }
}

/// Box filter with edge clamping (like `imageByClampingToExtent`).
fn box_pass(src: &[[f32; 3]], dst: &mut [[f32; 3]], w: usize, h: usize, r: usize, horizontal: bool) {
    let (len, lines) = if horizontal { (w, h) } else { (h, w) };
    if len == 0 {
        return;
    }
    let idx = |line: usize, i: usize| if horizontal { line * w + i } else { i * w + line };
    let norm = 1.0 / (2 * r + 1) as f32;
    let ri = r as isize;
    for line in 0..lines {
        let at = |i: isize| src[idx(line, i.clamp(0, len as isize - 1) as usize)];
        let mut acc = [0f32; 3];
        for i in -ri..=ri {
            let p = at(i);
            for c in 0..3 {
                acc[c] += p[c];
            }
        }
        for i in 0..len as isize {
            let out = &mut dst[idx(line, i as usize)];
            for c in 0..3 {
                out[c] = acc[c] * norm;
            }
            let (add, sub) = (at(i + ri + 1), at(i - ri));
            for c in 0..3 {
                acc[c] += add[c] - sub[c];
            }
        }
    }
}

/// Natural cubic spline through `points` (x ascending), like CIToneCurve.
struct Spline {
    x: Vec<f32>,
    y: Vec<f32>,
    m: Vec<f32>,
}

impl Spline {
    fn new(points: &[(f32, f32)]) -> Spline {
        let mut pts: Vec<(f32, f32)> = points.to_vec();
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
        let n = pts.len();
        let x: Vec<f32> = pts.iter().map(|p| p.0).collect();
        let y: Vec<f32> = pts.iter().map(|p| p.1).collect();
        let mut m = vec![0f32; n];
        if n > 2 {
            // Solve the tridiagonal system for second derivatives.
            let mut c = vec![0f32; n];
            let mut d = vec![0f32; n];
            for i in 1..n - 1 {
                let h0 = x[i] - x[i - 1];
                let h1 = x[i + 1] - x[i];
                let a = h0 / 6.0;
                let b = (h0 + h1) / 3.0;
                let cc = h1 / 6.0;
                let rhs = (y[i + 1] - y[i]) / h1 - (y[i] - y[i - 1]) / h0;
                let denom = b - a * c[i - 1];
                c[i] = cc / denom;
                d[i] = (rhs - a * d[i - 1]) / denom;
            }
            for i in (1..n - 1).rev() {
                m[i] = d[i] - c[i] * m[i + 1];
            }
        }
        Spline { x, y, m }
    }

    fn eval(&self, t: f32) -> f32 {
        let n = self.x.len();
        if n == 0 {
            return t;
        }
        if t <= self.x[0] {
            return self.y[0];
        }
        if t >= self.x[n - 1] {
            return self.y[n - 1];
        }
        let i = self.x.windows(2).position(|w| t <= w[1]).unwrap_or(n - 2);
        let (x0, x1) = (self.x[i], self.x[i + 1]);
        let h = x1 - x0;
        let a = (x1 - t) / h;
        let b = (t - x0) / h;
        a * self.y[i] + b * self.y[i + 1] + ((a * a * a - a) * self.m[i] + (b * b * b - b) * self.m[i + 1]) * h * h / 6.0
    }
}

/// Approximate sRGB-linear white point of a black body at `kelvin`.
fn white_point(kelvin: f32) -> [f32; 3] {
    let t = (kelvin / 100.0).clamp(10.0, 400.0);
    let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
    let g = if t <= 66.0 { 99.470_8 * t.ln() - 161.119_57 } else { 288.122_16 * (t - 60.0).powf(-0.075_514_85) };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };
    [to_linear((r / 255.0).clamp(0.0, 1.0)), to_linear((g / 255.0).clamp(0.0, 1.0)), to_linear((b / 255.0).clamp(0.0, 1.0))]
}

/// A small deterministic hash for grain.
fn noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ 0x9e37_79b9;
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

fn soft_light(base: f32, blend: f32) -> f32 {
    if blend <= 0.5 {
        base - (1.0 - 2.0 * blend) * base * (1.0 - base)
    } else {
        let d = if base <= 0.25 { ((16.0 * base - 12.0) * base + 4.0) * base } else { base.sqrt() };
        base + (2.0 * blend - 1.0) * (d - base)
    }
}

/// Applies `a` to `img` and renders the result at the same size.
pub fn apply(img: &RgbaImage, a: &Adjustments) -> Result<Image> {
    if a.is_identity() {
        return Ok(Arc::new(img.clone()));
    }
    let (w, h) = imaging::size(img);
    let long = w.max(h) as f32;
    let v = |k: &str| (a.get(k) / 100.0) as f32;
    let mut p = Planes::from(img);

    let ev = v("exposure") * 2.0;
    if ev != 0.0 {
        let gain = 2f32.powf(ev);
        p.map(|c| c.map(|x| to_srgb(to_linear(x) * gain)));
    }

    let (hi, sh, wh, bl, fade) = (v("highlights"), v("shadows"), v("whites"), v("blacks"), v("fade"));
    if hi != 0.0 || sh != 0.0 || wh != 0.0 || bl != 0.0 || fade != 0.0 {
        let p0 = ((-bl).max(0.0) * 0.12, bl.max(0.0) * 0.12 + fade * 0.15);
        let p1 = (0.25, (0.25 + sh * 0.12 + fade * 0.08).clamp(0.0, 1.0));
        let p2 = (0.5, (0.5 + (sh + hi) * 0.03).clamp(0.0, 1.0));
        let p3 = (0.75, (0.75 + hi * 0.12).clamp(0.0, 1.0));
        let p4 = (1.0 - wh.max(0.0) * 0.12, 1.0 - (-wh).max(0.0) * 0.12 - fade * 0.04);
        let curve = Spline::new(&[p0, p1, p2, p3, p4]);
        let lut: Vec<f32> = (0..=1024).map(|i| curve.eval(i as f32 / 1024.0).clamp(0.0, 1.0)).collect();
        p.map(|c| c.map(|x| lut[(x.clamp(0.0, 1.0) * 1024.0).round() as usize]));
    }

    let (br, co, sa, dh) = (v("brightness"), v("contrast"), v("saturation"), v("dehaze"));
    if br != 0.0 || co != 0.0 || sa != 0.0 || dh != 0.0 {
        let brightness = br * 0.2 - dh * 0.03;
        let contrast = (1.0 + co * 0.4 + dh * 0.25).max(0.25);
        let saturation = (1.0 + sa + dh * 0.15).max(0.0);
        p.map(|c| {
            let l = luma(c);
            let s = c.map(|x| l + (x - l) * saturation);
            s.map(|x| ((x + brightness) - 0.5) * contrast + 0.5)
        });
    }

    let gamma = v("gamma");
    if gamma != 0.0 {
        let power = 1.0 - gamma * 0.5;
        p.map(|c| c.map(|x| x.max(0.0).powf(power)));
    }

    let (temp, tint) = (v("temperature"), v("tint"));
    if temp != 0.0 || tint != 0.0 {
        // Lowering the target white point warms; a positive tint leans magenta.
        let neutral = white_point(6500.0);
        let target = white_point(6500.0 - temp * 3000.0);
        let mut gain = [target[0] / neutral[0], target[1] / neutral[1], target[2] / neutral[2]];
        gain[1] *= 1.0 - tint * 60.0 * 0.004;
        let norm = luma(gain);
        let gain = gain.map(|g| g / norm);
        p.map(|c| {
            let lin = c.map(to_linear);
            [to_srgb(lin[0] * gain[0]), to_srgb(lin[1] * gain[1]), to_srgb(lin[2] * gain[2])]
        });
    }

    let vibrance = v("vibrance");
    if vibrance != 0.0 {
        p.map(|c| {
            let max = c[0].max(c[1]).max(c[2]);
            let min = c[0].min(c[1]).min(c[2]);
            let sat = max - min;
            let amount = 1.0 + vibrance * (1.0 - sat);
            let l = luma(c);
            c.map(|x| l + (x - l) * amount)
        });
    }

    for (key, radius, strength) in [("clarity", 0.02f32, 0.6f32), ("texture", 0.004, 0.9)] {
        let k = v(key);
        if k > 0.0 {
            let blur = p.blurred((long * radius).max(1.0), |c| c);
            for (c, b) in p.rgb.iter_mut().zip(blur) {
                for i in 0..3 {
                    c[i] += (c[i] - b[i]) * k * strength;
                }
            }
        }
    }

    let nr = v("noiseReduction");
    if nr > 0.0 {
        // Edge-preserving smoothing: blend toward a small blur where the difference is noise-sized.
        let blur = p.blurred((long / 1000.0).max(1.0), |c| c);
        let level = nr * 0.06 * 4.0;
        for (c, b) in p.rgb.iter_mut().zip(blur) {
            for i in 0..3 {
                let d = c[i] - b[i];
                let wgt = (1.0 - (d.abs() / level.max(1e-4)).min(1.0)) * nr.min(1.0);
                c[i] -= d * wgt;
            }
        }
    }

    let sharp = v("sharpness");
    if sharp > 0.0 {
        let blur = p.blurred((long / 1000.0 * 1.5).max(1.0), |c| {
            let l = luma(c);
            [l, l, l]
        });
        for (c, b) in p.rgb.iter_mut().zip(blur) {
            let detail = luma(*c) - b[0];
            for x in c.iter_mut() {
                *x += detail * sharp * 1.2;
            }
        }
    }

    let vignette = v("vignette");
    if vignette != 0.0 {
        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
        let radius = ((w * w + h * h).sqrt() as f32) * 0.42;
        let falloff = 0.6;
        let width = p.w;
        for (i, c) in p.rgb.iter_mut().enumerate() {
            let (x, y) = ((i % width) as f32 + 0.5, (i / width) as f32 + 0.5);
            let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() / radius;
            let t = ((d - (1.0 - falloff)) / falloff).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            let factor = 1.0 - vignette * t;
            for x in c.iter_mut() {
                *x *= factor;
            }
        }
    }

    let grain = v("grain");
    if grain > 0.0 {
        let s = (long / 1500.0).max(1.0);
        let alpha = grain * 0.35;
        let width = p.w;
        for (i, c) in p.rgb.iter_mut().enumerate() {
            let (x, y) = ((i % width) as f32 / s, (i / width) as f32 / s);
            let n = noise(x as u32, y as u32);
            for ch in c.iter_mut() {
                let base = ch.clamp(0.0, 1.0);
                *ch = base + (soft_light(base, n) - base) * alpha;
            }
        }
    }

    Ok(Arc::new(p.into_image()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grey(level: f64) -> RgbaImage {
        let l = (level * 255.0).round() as u8;
        RgbaImage::from_pixel(64, 64, image::Rgba([l, l, l, 255]))
    }

    fn mean(img: &RgbaImage) -> (f64, f64, f64) {
        let n = (img.width() * img.height()) as f64;
        let sum = |c: usize| img.pixels().map(|p| p.0[c] as f64).sum::<f64>() / n;
        (sum(0), sum(1), sum(2))
    }

    fn with(key: &str, value: f64) -> Adjustments {
        let mut a = Adjustments::default();
        a.set(Adjustments::index(key).unwrap(), value);
        a
    }

    #[test]
    fn identity_returns_the_same_pixels() {
        let img = grey(0.5);
        let out = apply(&img, &Adjustments::default()).unwrap();
        assert_eq!(mean(&out), mean(&img));
    }

    #[test]
    fn light_adjustments_move_brightness_the_right_way() {
        let img = grey(0.5);
        let base = mean(&img).0;
        for key in ["exposure", "brightness", "shadows", "highlights", "gamma"] {
            let up = mean(&apply(&img, &with(key, 60.0)).unwrap()).0;
            let down = mean(&apply(&img, &with(key, -60.0)).unwrap()).0;
            assert!(up > base + 2.0, "{key} +: {up} vs {base}");
            assert!(down < base - 2.0, "{key} -: {down} vs {base}");
        }
        assert!(mean(&apply(&grey(0.05), &with("fade", 80.0)).unwrap()).0 > mean(&grey(0.05)).0 + 5.0);
    }

    #[test]
    fn temperature_warms_and_cools() {
        let img = grey(0.5);
        let (r, _, b) = mean(&apply(&img, &with("temperature", 60.0)).unwrap());
        assert!(r > b + 3.0, "warm: r={r} b={b}");
        let (r, _, b) = mean(&apply(&img, &with("temperature", -60.0)).unwrap());
        assert!(b > r + 3.0, "cool: r={r} b={b}");
        let (r, g, b) = mean(&apply(&img, &with("tint", 60.0)).unwrap());
        let (r2, g2, b2) = mean(&apply(&img, &with("tint", -60.0)).unwrap());
        assert!(g < (r + b) / 2.0 && g2 > (r2 + b2) / 2.0, "tint+ {r},{g},{b} tint- {r2},{g2},{b2}");
    }

    #[test]
    fn effects_render_at_full_size() {
        let img = grey(0.5);
        let mut a = Adjustments::default();
        for key in ["clarity", "texture", "sharpness", "noiseReduction", "vignette", "grain", "dehaze", "vibrance", "tint", "saturation", "contrast", "whites", "blacks"] {
            a.set(Adjustments::index(key).unwrap(), 50.0);
        }
        let out = apply(&img, &a).unwrap();
        assert_eq!(imaging::size(&out), (64.0, 64.0));
        let v = apply(&img, &with("vignette", 100.0)).unwrap();
        assert!(mean(&v).0 < mean(&img).0);
        assert!(v.get_pixel(0, 0).0[0] < v.get_pixel(32, 32).0[0]);
    }
}
