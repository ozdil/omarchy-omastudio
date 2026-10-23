//! Spot healing and cloning on the linear input, before any other stage.
//!
//! Each spot copies a disc of the frame from a source position onto a target
//! position. Cloning copies it as it is. Healing also matches the light and colour
//! of the surroundings: the ratio between the target's and the source's edges is
//! spread over the disc with the Poisson kernel, which is the smooth (harmonic)
//! interpolation of the edge values, so the copy takes on the target's shading.
//!
//! Positions are normalised to the frame (x across the width, y down the height)
//! and the radius is a share of the frame width, so spots work at any resolution.
//! Spots apply in order; a later spot sees the result of the earlier ones.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetouchMode {
    #[default]
    Heal,
    Clone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RetouchSpot {
    pub mode: RetouchMode,
    /// Centre of the area that is replaced
    pub x: f32,
    pub y: f32,
    /// Centre of the area copied from
    pub source_x: f32,
    pub source_y: f32,
    /// Radius in frame widths
    pub radius: f32,
    /// Soft edge as a share of the radius (0 hard .. 1 fully soft)
    pub feather: f32,
    pub opacity: f32,
}

impl Default for RetouchSpot {
    fn default() -> Self {
        Self { mode: RetouchMode::Heal, x: 0.5, y: 0.5, source_x: 0.6, source_y: 0.5, radius: 0.02, feather: 0.3, opacity: 1.0 }
    }
}

/// Edge samples taken around each disc for healing
const EDGE_SAMPLES: usize = 64;

/// The input with the spots applied, or the input itself when there are none.
pub fn apply<'a>(input: &'a [u16], w: usize, h: usize, ch: usize, spots: &[RetouchSpot]) -> Cow<'a, [u16]> {
    let spots: Vec<&RetouchSpot> = spots.iter().filter(|s| is_usable(s)).collect();
    if spots.is_empty() || w < 2 || h < 2 || input.len() < w * h * ch {
        return Cow::Borrowed(input);
    }
    let mut out = input.to_vec();
    for spot in spots {
        apply_spot(&mut out, w, h, ch, spot);
    }
    Cow::Owned(out)
}

fn is_usable(s: &RetouchSpot) -> bool {
    [s.x, s.y, s.source_x, s.source_y, s.radius, s.feather, s.opacity].iter().all(|v| v.is_finite())
        && s.radius > 0.0
        && s.opacity > 0.0
}

/// Reads a pixel channel with the position clamped into the frame.
#[inline]
fn at(buf: &[u16], w: usize, h: usize, ch: usize, x: f32, y: f32, c: usize) -> f32 {
    let xi = (x.round().max(0.0) as usize).min(w - 1);
    let yi = (y.round().max(0.0) as usize).min(h - 1);
    buf[(yi * w + xi) * ch + c] as f32
}

/// Mean of a 3x3 neighbourhood, so one noisy pixel does not set an edge value.
fn edge_mean(buf: &[u16], w: usize, h: usize, ch: usize, x: f32, y: f32, c: usize) -> f32 {
    let mut sum = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            sum += at(buf, w, h, ch, x + dx as f32, y + dy as f32, c);
        }
    }
    sum / 9.0
}

fn apply_spot(buf: &mut [u16], w: usize, h: usize, ch: usize, spot: &RetouchSpot) {
    let r = (spot.radius * w as f32).clamp(1.0, w.max(h) as f32);
    let (tx, ty) = (spot.x * w as f32, spot.y * h as f32);
    let (sx, sy) = (spot.source_x * w as f32, spot.source_y * h as f32);
    let (dx, dy) = (sx - tx, sy - ty);
    let feather = spot.feather.clamp(0.0, 1.0);
    let opacity = spot.opacity.clamp(0.0, 1.0);
    let heal = spot.mode == RetouchMode::Heal;

    // Healing: log ratio of target to source on the edge, per channel. The edge sits
    // just outside the disc, on pixels the spot does not replace.
    let edge_r = r + 1.5;
    let mut edge = vec![[0.0f32; 3]; if heal { EDGE_SAMPLES } else { 0 }];
    let mut edge_dir = vec![(0.0f32, 0.0f32); edge.len()];
    for (k, (e, d)) in edge.iter_mut().zip(edge_dir.iter_mut()).enumerate() {
        let a = k as f32 / EDGE_SAMPLES as f32 * std::f32::consts::TAU;
        let (ca, sa) = (a.cos(), a.sin());
        *d = (ca * r, sa * r);
        for (c, v) in e.iter_mut().enumerate() {
            let t = edge_mean(buf, w, h, ch, tx + ca * edge_r, ty + sa * edge_r, c);
            let s = edge_mean(buf, w, h, ch, sx + ca * edge_r, sy + sa * edge_r, c);
            *v = ((t + 16.0) / (s + 16.0)).ln();
        }
    }

    let x0 = (tx - r).floor().max(0.0) as usize;
    let x1 = ((tx + r).ceil().max(0.0) as usize).min(w - 1);
    let y0 = (ty - r).floor().max(0.0) as usize;
    let y1 = ((ty + r).ceil().max(0.0) as usize).min(h - 1);
    if x0 > x1 || y0 > y1 {
        return;
    }

    // The source is read from a copy, so a source overlapping the target reads the
    // pixels as they were before this spot
    let (sx0, sx1) = (((x0 as f32 + dx).floor().max(0.0) as usize).min(w - 1), ((x1 as f32 + dx).ceil().max(0.0) as usize).min(w - 1));
    let (sy0, sy1) = (((y0 as f32 + dy).floor().max(0.0) as usize).min(h - 1), ((y1 as f32 + dy).ceil().max(0.0) as usize).min(h - 1));
    let sw = sx1 - sx0 + 1;
    let mut src = Vec::with_capacity(sw * (sy1 - sy0 + 1) * 3);
    for y in sy0..=sy1 {
        for x in sx0..=sx1 {
            let k = (y * w + x) * ch;
            src.extend_from_slice(&buf[k..k + 3]);
        }
    }
    let src_at = |x: f32, y: f32, c: usize| -> f32 {
        let xi = (x.round().max(0.0) as usize).clamp(sx0, sx1) - sx0;
        let yi = (y.round().max(0.0) as usize).clamp(sy0, sy1) - sy0;
        src[(yi * sw + xi) * 3 + c] as f32
    };

    let inner = 1.0 - feather;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (px, py) = (x as f32 - tx, y as f32 - ty);
            let d = (px * px + py * py).sqrt() / r;
            if d >= 1.0 {
                continue;
            }
            let alpha = if d <= inner {
                1.0
            } else {
                let t = ((1.0 - d) / feather.max(1e-4)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            } * opacity;

            // Poisson kernel weights for this point, shared by the three channels
            let mut corr = [0.0f32; 3];
            if heal {
                let rr = r * r;
                let p2 = px * px + py * py;
                let mut wsum = 0.0;
                for (e, &(ex, ey)) in edge.iter().zip(edge_dir.iter()) {
                    let q = (px - ex) * (px - ex) + (py - ey) * (py - ey);
                    let wk = (rr - p2).max(0.0) / q.max(1e-3);
                    wsum += wk;
                    for c in 0..3 {
                        corr[c] += wk * e[c];
                    }
                }
                for v in corr.iter_mut() {
                    *v = (*v / wsum.max(1e-6)).exp();
                }
            }

            let k = (y * w + x) * ch;
            for c in 0..3 {
                let s = src_at(x as f32 + dx, y as f32 + dy, c);
                let s = if heal { (s + 16.0) * corr[c] - 16.0 } else { s };
                let v = buf[k + c] as f32 * (1.0 - alpha) + s * alpha;
                buf[k + c] = v.round().clamp(0.0, 65535.0) as u16;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smooth horizontal gradient with a dark blemish at (20, 20)
    fn frame(w: usize, h: usize) -> Vec<u16> {
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let base = 8000.0 + x as f32 * 300.0;
                let blemish = ((x as f32 - 20.0).powi(2) + (y as f32 - 20.0).powi(2)).sqrt() < 3.0;
                let p = if blemish { 500.0 } else { base };
                v.extend_from_slice(&[p as u16, (p * 0.8) as u16, (p * 0.6) as u16]);
            }
        }
        v
    }

    fn px(buf: &[u16], w: usize, x: usize, y: usize) -> [u16; 3] {
        let k = (y * w + x) * 3;
        [buf[k], buf[k + 1], buf[k + 2]]
    }

    #[test]
    fn no_spots_borrows_the_input() {
        let f = frame(40, 40);
        assert!(matches!(apply(&f, 40, 40, 3, &[]), Cow::Borrowed(_)));
    }

    #[test]
    fn heal_removes_a_blemish_and_keeps_the_gradient() {
        let (w, h) = (64, 40);
        let f = frame(w, h);
        // The source is 15 px to the right, where the gradient is brighter
        let spot = RetouchSpot {
            x: 20.5 / w as f32,
            y: 20.5 / h as f32,
            source_x: 35.5 / w as f32,
            source_y: 20.5 / h as f32,
            radius: 6.0 / w as f32,
            feather: 0.2,
            ..Default::default()
        };
        let out = apply(&f, w, h, 3, std::slice::from_ref(&spot));
        let expect = 8000.0 + 20.0 * 300.0;
        let got = px(&out, w, 20, 20)[0] as f32;
        assert!((got - expect).abs() < expect * 0.03, "healed {got}, expected about {expect}");

        // Cloning carries the source's brighter values over unchanged
        let clone = RetouchSpot { mode: RetouchMode::Clone, ..spot };
        let out = apply(&f, w, h, 3, &[clone]);
        let got = px(&out, w, 20, 20)[0] as f32;
        let src = 8000.0 + 35.0 * 300.0;
        assert!((got - src).abs() < 30.0, "cloned {got}, expected {src}");
    }

    #[test]
    fn pixels_outside_the_disc_are_untouched() {
        let (w, h) = (64, 40);
        let f = frame(w, h);
        let spot = RetouchSpot { x: 0.3, y: 0.5, source_x: 0.6, source_y: 0.5, radius: 0.05, ..Default::default() };
        let out = apply(&f, w, h, 3, &[spot]);
        let r = 0.05 * w as f32;
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f32 - 0.3 * w as f32).powi(2) + (y as f32 - 0.5 * h as f32).powi(2)).sqrt();
                if d >= r + 0.5 {
                    assert_eq!(px(&out, w, x, y), px(&f, w, x, y), "({x}, {y}) changed");
                }
            }
        }
    }

    #[test]
    fn spots_at_the_frame_edge_and_bad_values_are_safe() {
        let (w, h) = (32, 24);
        let f = frame(w, h);
        let spots = [
            RetouchSpot { x: 0.0, y: 1.0, source_x: 1.0, source_y: 0.0, radius: 0.3, ..Default::default() },
            RetouchSpot { x: f32::NAN, ..Default::default() },
            RetouchSpot { radius: 0.0, ..Default::default() },
        ];
        let out = apply(&f, w, h, 3, &spots);
        assert_eq!(out.len(), f.len());
    }

    #[test]
    fn spots_round_trip_through_json() {
        let s = RetouchSpot { mode: RetouchMode::Clone, x: 0.1, ..Default::default() };
        let back: RetouchSpot = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let partial: RetouchSpot = serde_json::from_str(r#"{"x":0.2,"y":0.3}"#).unwrap();
        assert_eq!(partial.mode, RetouchMode::Heal);
        assert_eq!(partial.radius, 0.02);
    }
}
