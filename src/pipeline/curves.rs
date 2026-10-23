//! Point curves: a master RGB curve and one curve per channel, each a list of
//! control points on display-encoded values in 0..1, interpolated with a
//! monotone cubic (Fritsch–Carlson) so the curve never overshoots its points.

use crate::recipe::Recipe;

/// Samples per curve table; lookups interpolate linearly between them.
const TABLE_SIZE: usize = 1024;

/// One curve evaluated into a table.
#[derive(Clone, Debug)]
pub struct CurveTable {
    table: Vec<f32>,
}

impl CurveTable {
    /// Builds a table from control points, or None when the points describe the
    /// identity (no points, or only the two corners).
    pub fn new(points: &[[f32; 2]]) -> Option<Self> {
        let pts = normalise(points);
        if pts.len() < 2 || is_identity(&pts) {
            return None;
        }
        let tangents = monotone_tangents(&pts);
        let table = (0..TABLE_SIZE)
            .map(|i| hermite(&pts, &tangents, i as f32 / (TABLE_SIZE - 1) as f32))
            .collect();
        Some(Self { table })
    }

    /// Curve value at `x`. Outside 0..1 the curve continues with slope 1 from its
    /// end values, so highlight headroom above 1.0 is kept.
    #[inline(always)]
    pub fn apply(&self, x: f32) -> f32 {
        let last = TABLE_SIZE - 1;
        if !x.is_finite() {
            return x;
        }
        if x <= 0.0 {
            return self.table[0] + x;
        }
        if x >= 1.0 {
            return self.table[last] + (x - 1.0);
        }
        let pos = x * last as f32;
        let i = (pos as usize).min(last - 1);
        let t = pos - i as f32;
        self.table[i] + (self.table[i + 1] - self.table[i]) * t
    }
}

/// The master and per-channel curves of a recipe.
#[derive(Clone, Debug, Default)]
pub struct Curves {
    master: Option<CurveTable>,
    red: Option<CurveTable>,
    green: Option<CurveTable>,
    blue: Option<CurveTable>,
}

impl Curves {
    pub fn from_recipe(recipe: &Recipe) -> Self {
        Self {
            master: CurveTable::new(&recipe.curve_points),
            red: CurveTable::new(&recipe.curve_points_red),
            green: CurveTable::new(&recipe.curve_points_green),
            blue: CurveTable::new(&recipe.curve_points_blue),
        }
    }

    pub fn is_identity(&self) -> bool {
        self.master.is_none() && self.red.is_none() && self.green.is_none() && self.blue.is_none()
    }

    /// Master curve on every channel, then each channel's own curve.
    #[inline(always)]
    pub fn apply(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        let (mut r, mut g, mut b) = (r, g, b);
        if let Some(m) = &self.master {
            r = m.apply(r);
            g = m.apply(g);
            b = m.apply(b);
        }
        if let Some(c) = &self.red {
            r = c.apply(r);
        }
        if let Some(c) = &self.green {
            g = c.apply(g);
        }
        if let Some(c) = &self.blue {
            b = c.apply(b);
        }
        (r, g, b)
    }
}

/// Clamps points to 0..1, sorts them by x and drops points sharing an x.
fn normalise(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut pts: Vec<[f32; 2]> = points
        .iter()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
        .map(|p| [p[0].clamp(0.0, 1.0), p[1].clamp(0.0, 1.0)])
        .collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    pts.dedup_by(|b, a| (b[0] - a[0]).abs() < 1e-4);
    pts
}

fn is_identity(pts: &[[f32; 2]]) -> bool {
    pts.iter().all(|p| (p[0] - p[1]).abs() < 1e-4) && pts[0][0] < 1e-4 && pts[pts.len() - 1][0] > 1.0 - 1e-4
}

/// Fritsch–Carlson tangents: secant averages, flattened where the data turns
/// and limited so each segment stays monotone.
fn monotone_tangents(pts: &[[f32; 2]]) -> Vec<f32> {
    let n = pts.len();
    let secants: Vec<f32> = pts.windows(2).map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0])).collect();
    let mut m = vec![0.0f32; n];
    m[0] = secants[0];
    m[n - 1] = secants[n - 2];
    for i in 1..n - 1 {
        m[i] = if secants[i - 1] * secants[i] <= 0.0 { 0.0 } else { (secants[i - 1] + secants[i]) * 0.5 };
    }
    for i in 0..n - 1 {
        let d = secants[i];
        if d == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let a = m[i] / d;
        let b = m[i + 1] / d;
        let s = a * a + b * b;
        if s > 9.0 {
            let t = 3.0 / s.sqrt();
            m[i] = t * a * d;
            m[i + 1] = t * b * d;
        }
    }
    m
}

/// Hermite evaluation; flat before the first point and after the last.
fn hermite(pts: &[[f32; 2]], m: &[f32], x: f32) -> f32 {
    let n = pts.len();
    if x <= pts[0][0] {
        return pts[0][1];
    }
    if x >= pts[n - 1][0] {
        return pts[n - 1][1];
    }
    let i = pts.windows(2).position(|w| x < w[1][0]).unwrap_or(n - 2);
    let (x0, y0) = (pts[i][0], pts[i][1]);
    let (x1, y1) = (pts[i + 1][0], pts[i + 1][1]);
    let h = x1 - x0;
    let t = (x - x0) / h;
    let (t2, t3) = (t * t, t * t * t);
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    (h00 * y0 + h10 * h * m[i] + h01 * y1 + h11 * h * m[i + 1]).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_only_is_identity() {
        assert!(CurveTable::new(&[]).is_none());
        assert!(CurveTable::new(&[[0.0, 0.0], [1.0, 1.0]]).is_none());
        assert!(CurveTable::new(&[[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]]).is_none());
    }

    #[test]
    fn passes_through_points_and_stays_monotone() {
        let pts = [[0.0, 0.0], [0.25, 0.15], [0.5, 0.5], [0.75, 0.85], [1.0, 1.0]];
        let c = CurveTable::new(&pts).unwrap();
        for p in pts {
            assert!((c.apply(p[0]) - p[1]).abs() < 2e-3, "{p:?} -> {}", c.apply(p[0]));
        }
        let mut prev = -1.0;
        for i in 0..=1000 {
            let y = c.apply(i as f32 / 1000.0);
            assert!(y >= prev - 1e-6);
            prev = y;
        }
    }

    #[test]
    fn no_overshoot_on_steep_step() {
        let c = CurveTable::new(&[[0.0, 0.0], [0.45, 0.05], [0.55, 0.95], [1.0, 1.0]]).unwrap();
        for i in 0..=1000 {
            let y = c.apply(i as f32 / 1000.0);
            assert!((0.0..=1.0).contains(&y));
        }
    }

    #[test]
    fn black_point_and_headroom() {
        // Lifted black, lowered white: ends are flat inside, slope 1 outside.
        let c = CurveTable::new(&[[0.0, 0.1], [1.0, 0.9]]).unwrap();
        assert!((c.apply(0.0) - 0.1).abs() < 1e-4);
        assert!((c.apply(1.0) - 0.9).abs() < 1e-4);
        assert!((c.apply(1.2) - 1.1).abs() < 1e-4);
        // Points not at the corners hold their value to the edge.
        let c = CurveTable::new(&[[0.1, 0.0], [0.9, 1.0]]).unwrap();
        assert_eq!(c.apply(0.05), 0.0);
        assert_eq!(c.apply(0.95), 1.0);
    }

    #[test]
    fn channel_curves_apply_after_master() {
        let r = Recipe { curve_points_red: vec![[0.0, 0.0], [0.5, 0.6], [1.0, 1.0]], ..Recipe::default() };
        let curves = Curves::from_recipe(&r);
        let (cr, cg, cb) = curves.apply(0.5, 0.5, 0.5);
        assert!((cr - 0.6).abs() < 2e-3 && cg == 0.5 && cb == 0.5);
        assert!(Curves::from_recipe(&Recipe::default()).is_identity());
    }
}
