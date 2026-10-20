//! The wheel's liquid highlight as a small physical system, after 30nama's pill.
//!
//! Each edge of the blob is a spring chasing the hovered petal's edge. The edge in the
//! direction of travel is stiffer than the trailing one, so the blob stretches as it goes and
//! gathers itself when it arrives; it keeps its area, so it thins while stretched. When the
//! hover moves on, the blob leaves a drop behind that shrinks away; while the two are close
//! their distance fields melt together into a neck that pinches off. Retargeting mid-flow
//! simply changes where the springs pull, so the motion never jumps.

use std::f32::consts::{PI, TAU};
use std::time::Instant;

/// A ring sector: centre angle (radians, y up), half its angle, mid radius and half its
/// thickness (DIPs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sector {
    pub angle: f32,
    pub half: f32,
    pub mid: f32,
    pub thick: f32,
}

/// A damped spring's response time (s) and damping ratio.
#[derive(Clone, Copy)]
struct Spring {
    response: f32,
    damping: f32,
}

/// The leading edge runs ahead…
const LEAD: Spring = Spring { response: 0.22, damping: 0.74 };
/// …and the trailing edge lets go later.
const TRAIL: Spring = Spring { response: 0.40, damping: 0.86 };
/// Growing in from nothing, or shrinking away.
const PRESENCE: Spring = Spring { response: 0.24, damping: 0.80 };
/// How long a left-behind drop takes to vanish (s).
const DROP_LIFE: f32 = 0.34;

fn step_spring(x: &mut f32, v: &mut f32, target: f32, s: Spring, dt: f32) {
    let w = TAU / s.response;
    let a = -w * w * (*x - target) - 2.0 * s.damping * w * *v;
    *v += a * dt;
    *x += *v * dt;
}

#[derive(Clone, Debug)]
struct Drop {
    shape: Sector,
    age: f32,
}

#[derive(Clone, Debug)]
pub struct Liquid {
    /// Edges of the blob (continuous angles: they may wind past ±π).
    lo: f32,
    hi: f32,
    v_lo: f32,
    v_hi: f32,
    /// 0 = gone, 1 = fully there.
    presence: f32,
    v_presence: f32,
    /// The petal being chased, as continuous edges, plus its shape.
    target: Option<(f32, f32, Sector)>,
    /// The last petal's shape, kept for shrinking away when the hover leaves.
    shape: Option<Sector>,
    drops: Vec<Drop>,
    last: Option<Instant>,
}

impl Default for Liquid {
    fn default() -> Self {
        Liquid { lo: 0.0, hi: 0.0, v_lo: 0.0, v_hi: 0.0, presence: 0.0, v_presence: 0.0, target: None, shape: None, drops: Vec::new(), last: None }
    }
}

/// `a` moved by whole turns to lie within half a turn of `near`.
fn near(a: f32, near: f32) -> f32 {
    a + ((near - a) / TAU).round() * TAU
}

impl Liquid {
    /// Chase `petal` (or fade away with `None`).
    pub fn set_target(&mut self, petal: Option<Sector>) {
        let Some(p) = petal else {
            self.target = None;
            return;
        };
        if self.presence < 0.02 || self.shape.is_none() {
            // Appear on the petal itself, growing from its middle.
            self.lo = p.angle;
            self.hi = p.angle;
            self.v_lo = 0.0;
            self.v_hi = 0.0;
            self.drops.clear();
        } else if let Some(body) = self.body() {
            // The blob sets off; a drop of it stays behind.
            self.drops.push(Drop { shape: body, age: 0.0 });
            self.drops.retain(|d| d.age < DROP_LIFE);
            if self.drops.len() > 3 {
                self.drops.remove(0);
            }
        }
        let centre = near(p.angle, (self.lo + self.hi) / 2.0);
        self.target = Some((centre - p.half, centre + p.half, p));
        self.shape = Some(p);
        self.last.get_or_insert_with(Instant::now);
    }

    /// Jumps straight to rest on `petal` (for still renders).
    pub fn settle_on(&mut self, petal: Sector) {
        *self = Liquid::default();
        self.set_target(Some(petal));
        if let Some((lo, hi, _)) = self.target {
            self.lo = lo;
            self.hi = hi;
        }
        self.presence = 1.0;
    }

    /// Advances the physics to `now`; returns whether anything is still moving.
    pub fn step(&mut self, now: Instant) -> bool {
        let dt = self.last.map(|t| (now - t).as_secs_f32()).unwrap_or(0.0).min(0.05);
        self.last = Some(now);
        self.advance(dt)
    }

    /// Advances the physics by `dt` seconds.
    pub fn advance(&mut self, dt: f32) -> bool {
        // Small sub-steps keep the stiff springs stable at any frame rate.
        let steps = (dt / 0.004).ceil().max(1.0) as usize;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let goal = if self.target.is_some() { 1.0 } else { 0.0 };
            step_spring(&mut self.presence, &mut self.v_presence, goal, PRESENCE, h);
            if let Some((t_lo, t_hi, _)) = self.target {
                let forward = (t_lo + t_hi) / 2.0 > (self.lo + self.hi) / 2.0;
                let (s_lo, s_hi) = if forward { (TRAIL, LEAD) } else { (LEAD, TRAIL) };
                step_spring(&mut self.lo, &mut self.v_lo, t_lo, s_lo, h);
                step_spring(&mut self.hi, &mut self.v_hi, t_hi, s_hi, h);
                if self.hi < self.lo {
                    let m = (self.lo + self.hi) / 2.0;
                    self.lo = m;
                    self.hi = m;
                }
            }
        }
        for d in &mut self.drops {
            d.age += dt;
        }
        self.drops.retain(|d| d.age < DROP_LIFE);
        self.presence = self.presence.clamp(0.0, 1.2);
        self.moving()
    }

    pub fn moving(&self) -> bool {
        let settled_edges = match self.target {
            Some((t_lo, t_hi, _)) => (self.lo - t_lo).abs() < 0.002 && (self.hi - t_hi).abs() < 0.002 && self.v_lo.abs() < 0.01 && self.v_hi.abs() < 0.01,
            None => true,
        };
        let goal = if self.target.is_some() { 1.0 } else { 0.0 };
        !(settled_edges && (self.presence - goal).abs() < 0.005 && self.v_presence.abs() < 0.01 && self.drops.is_empty())
    }

    /// The blob itself: keeps its area, so it thins as it stretches.
    fn body(&self) -> Option<Sector> {
        let p = self.shape?;
        if self.presence <= 0.01 {
            return None;
        }
        let g = self.presence.min(1.1);
        let half = ((self.hi - self.lo) / 2.0).max(0.0);
        // Growing in from nothing: the petal's width scales with presence.
        let half = half.max(p.half * 0.15) * if self.target.is_some() { 1.0 } else { g };
        let stretch = (half / p.half.max(1e-3)).max(1e-3);
        let thick = p.thick * (1.0 / stretch.sqrt()).clamp(0.55, 1.12) * g.min(1.0);
        Some(Sector { angle: (self.lo + self.hi) / 2.0, half: half * g.min(1.0).max(0.2), mid: p.mid, thick })
    }

    /// Everything to draw: the blob and the drops left behind (shrinking as they age).
    pub fn pieces(&self) -> Vec<Sector> {
        let mut out: Vec<Sector> = self.body().into_iter().collect();
        for d in &self.drops {
            let k = (1.0 - d.age / DROP_LIFE).clamp(0.0, 1.0).powf(1.6);
            if k > 0.03 {
                out.push(Sector { half: d.shape.half * k, thick: d.shape.thick * k, ..d.shape });
            }
        }
        out
    }
}

/// Angle difference wrapped into (−π, π].
pub fn wrap(a: f32) -> f32 {
    let mut a = a.rem_euclid(TAU);
    if a > PI {
        a -= TAU;
    }
    a
}

/// Signed distance to a sector with rounded corners, measured along arcs and radii.
pub fn sector_sdf(x: f32, y_up: f32, s: Sector) -> f32 {
    let r = (x * x + y_up * y_up).sqrt();
    let u = wrap(y_up.atan2(x) - s.angle).abs() * r;
    let corner = 11f32.min(s.half * s.mid * 0.5).min(s.thick * 0.6).max(0.5);
    let qx = u - (s.half * r - corner);
    let qy = (r - s.mid).abs() - (s.thick - corner);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - corner
}

/// A box (y down, around centre `c`) that holds every piece.
pub fn bounds(pieces: &[Sector], c: f32) -> Option<(f32, f32, f32, f32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for s in pieces {
        let steps = ((s.half * 2.0) / 0.15).ceil().clamp(2.0, 48.0) as usize;
        for k in 0..=steps {
            let a = s.angle - s.half + s.half * 2.0 * k as f32 / steps as f32;
            for r in [s.mid - s.thick, s.mid + s.thick] {
                let (x, y) = (c + r * a.cos(), c - r * a.sin());
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
    }
    // Room for the melt between pieces and anti-aliasing.
    let pad = super::gfx::REACH / 2.0 + 3.0;
    (x0 <= x1).then(|| (x0 - pad, y0 - pad, (x1 - x0) + pad * 2.0, (y1 - y0) + pad * 2.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn petal(angle: f32) -> Sector {
        Sector { angle, half: 0.35, mid: 83.0, thick: 37.0 }
    }

    #[test]
    fn flows_to_the_new_petal_and_settles() {
        let mut l = Liquid::default();
        l.settle_on(petal(0.0));
        l.set_target(Some(petal(1.5)));
        let mut stretched = false;
        for _ in 0..120 {
            l.advance(1.0 / 60.0);
            let p = l.pieces();
            if p[0].half > 0.5 {
                stretched = true;
            }
        }
        assert!(stretched, "it stretches on the way");
        assert!(!l.moving(), "and comes to rest");
        let body = l.pieces()[0];
        assert!((body.angle - 1.5).abs() < 0.01 && (body.half - 0.35).abs() < 0.01, "{body:?}");
    }

    #[test]
    fn takes_the_short_way_round_and_can_be_retargeted() {
        let mut l = Liquid::default();
        l.settle_on(petal(3.0));
        // −3.0 is just past ±π from 3.0: go forward through π, not back through 0.
        l.set_target(Some(petal(-3.0)));
        l.advance(0.05);
        l.set_target(Some(petal(-2.2)));
        for _ in 0..120 {
            l.advance(1.0 / 60.0);
        }
        let body = l.pieces()[0];
        assert!((wrap(body.angle - (-2.2))).abs() < 0.01, "{body:?}");
        assert!(body.angle > 3.0, "kept winding forward: {}", body.angle);
    }

    #[test]
    fn fades_away_when_the_hover_leaves() {
        let mut l = Liquid::default();
        l.settle_on(petal(0.0));
        l.set_target(None);
        for _ in 0..90 {
            l.advance(1.0 / 60.0);
        }
        assert!(l.pieces().is_empty() && !l.moving());
    }
}
