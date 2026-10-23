use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MaskShape {
    LinearGradient {
        start_x: f32, // 0.0 to 1.0 (normalized image coords)
        start_y: f32,
        end_x: f32,
        end_y: f32,
    },
    RadialGradient {
        center_x: f32, // 0.0 to 1.0
        center_y: f32,
        radius_x: f32, // 0.0 to 1.0
        radius_y: f32,
        feather: f32,  // 0.0 to 1.0 (Default: 0.5)
    },
    LumaRange {
        min_luma: f32, // 0.0 to 1.0
        max_luma: f32, // 0.0 to 1.0
        falloff: f32,  // 0.0 to 0.5
    },
    /// A painted or AI-generated mask: a grayscale image in [`masks_dir`], stretched
    /// over the whole frame
    Bitmap {
        path: String,
    },
    /// Brush strokes painted on the frame. Sizes are in frame widths; `aspect` is the
    /// frame's height / width, so strokes stay round.
    Brush {
        strokes: Vec<BrushStroke>,
        aspect: f32,
        #[serde(skip)]
        cache: BrushCache,
    },
    /// Colours near a target colour (display-encoded RGB), compared in OKLab.
    /// `range` and `feather` run 0..1 over a distance of 0..0.3 in OKLab.
    ColorRange {
        r: f32,
        g: f32,
        b: f32,
        range: f32,
        feather: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BrushStroke {
    /// Centre line in normalised frame coordinates
    pub points: Vec<[f32; 2]>,
    /// Radius in frame widths
    pub size: f32,
    /// Soft edge as a share of the radius (0 hard .. 1 fully soft)
    pub feather: f32,
    /// Strength of one stroke (0..1)
    pub flow: f32,
    /// Removes paint instead of adding it
    pub erase: bool,
}

impl Default for BrushStroke {
    fn default() -> Self {
        Self { points: Vec::new(), size: 0.05, feather: 0.5, flow: 1.0, erase: false }
    }
}

/// The rasterised strokes of a brush mask, built on first use for each recipe.
#[derive(Default)]
pub struct BrushCache(OnceLock<Arc<MaskBitmap>>);

impl Clone for BrushCache {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for BrushCache {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for BrushCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrushCache")
    }
}

/// How a component combines with the mask built so far.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    #[default]
    Add,
    Subtract,
    Intersect,
}

/// A further shape that adds to, subtracts from or intersects a layer's mask.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MaskComponent {
    pub mode: MaskMode,
    pub invert: bool,
    pub shape: MaskShape,
}

impl Default for MaskComponent {
    fn default() -> Self {
        Self { mode: MaskMode::Add, invert: false, shape: MaskShape::LumaRange { min_luma: 0.0, max_luma: 1.0, falloff: 0.1 } }
    }
}

/// Grid width of a rasterised brush mask; the height follows the frame's aspect.
const BRUSH_GRID: usize = 1024;

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Paints the strokes into a grid: painting adds coverage (a + (1 - a) * s), erasing
/// removes it (a * (1 - s)), stroke by stroke in order.
fn rasterize_brush(strokes: &[BrushStroke], aspect: f32) -> MaskBitmap {
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect.clamp(0.05, 20.0) } else { 1.0 };
    let w = BRUSH_GRID;
    let h = ((BRUSH_GRID as f32 * aspect).round() as usize).clamp(8, 4 * BRUSH_GRID);
    let mut mask = vec![0.0f32; w * h];
    let mut cover = vec![0.0f32; w * h];
    for stroke in strokes {
        if stroke.points.is_empty() || stroke.size <= 0.0 || stroke.flow <= 0.0 {
            continue;
        }
        let radius = stroke.size;
        let inner = radius * (1.0 - stroke.feather.clamp(0.0, 1.0));
        // Bounding box in grid cells; x in frame widths, y in frame heights
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for p in &stroke.points {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
        let gx0 = (((x0 - radius) * w as f32).floor().max(0.0)) as usize;
        let gx1 = (((x1 + radius) * w as f32).ceil().max(0.0) as usize).min(w);
        let gy0 = (((y0 - radius / aspect) * h as f32).floor().max(0.0)) as usize;
        let gy1 = (((y1 + radius / aspect) * h as f32).ceil().max(0.0) as usize).min(h);
        if gx0 >= gx1 || gy0 >= gy1 {
            continue;
        }
        for gy in gy0..gy1 {
            cover[gy * w + gx0..gy * w + gx1].fill(0.0);
        }
        // Distance in frame widths from each cell to each segment
        let segs: Vec<([f32; 2], [f32; 2])> = if stroke.points.len() == 1 {
            vec![(stroke.points[0], stroke.points[0])]
        } else {
            stroke.points.windows(2).map(|s| (s[0], s[1])).collect()
        };
        for (a, b) in segs {
            let (ax, ay, bx, by) = (a[0], a[1] * aspect, b[0], b[1] * aspect);
            let sx0 = (((ax.min(bx) - radius) * w as f32).floor().max(gx0 as f32)) as usize;
            let sx1 = (((ax.max(bx) + radius) * w as f32).ceil().max(0.0) as usize).min(gx1);
            let sy0 = (((ay.min(by) - radius) / aspect * h as f32).floor().max(gy0 as f32)) as usize;
            let sy1 = (((ay.max(by) + radius) / aspect * h as f32).ceil().max(0.0) as usize).min(gy1);
            let (dx, dy) = (bx - ax, by - ay);
            let len_sq = dx * dx + dy * dy;
            for gy in sy0..sy1 {
                let py = (gy as f32 + 0.5) / h as f32 * aspect;
                for gx in sx0..sx1 {
                    let px = (gx as f32 + 0.5) / w as f32;
                    let t = if len_sq > 0.0 { (((px - ax) * dx + (py - ay) * dy) / len_sq).clamp(0.0, 1.0) } else { 0.0 };
                    let (qx, qy) = (px - (ax + t * dx), py - (ay + t * dy));
                    let d = (qx * qx + qy * qy).sqrt();
                    if d >= radius {
                        continue;
                    }
                    let a = if d <= inner { 1.0 } else { smoothstep((radius - d) / (radius - inner).max(1e-6)) };
                    let c = &mut cover[gy * w + gx];
                    *c = c.max(a);
                }
            }
        }
        let flow = stroke.flow.clamp(0.0, 1.0);
        for gy in gy0..gy1 {
            for gx in gx0..gx1 {
                let i = gy * w + gx;
                let s = cover[i] * flow;
                mask[i] = if stroke.erase { mask[i] * (1.0 - s) } else { mask[i] + (1.0 - mask[i]) * s };
            }
        }
    }
    MaskBitmap { width: w, height: h, data: mask.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect() }
}

/// Display-encoded sRGB to OKLab (decoded with a 2.2 power).
fn oklab(r: f32, g: f32, b: f32) -> [f32; 3] {
    let lin = |v: f32| v.max(0.0).powf(2.2);
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// Where mask images live; a Bitmap path outside it is ignored.
pub fn masks_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/omastudio/masks")
}

pub struct MaskBitmap {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl MaskBitmap {
    /// Bilinear weight at normalised frame coordinates
    fn sample(&self, u: f32, v: f32) -> f32 {
        let x = (u.clamp(0.0, 1.0) * (self.width - 1) as f32).max(0.0);
        let y = (v.clamp(0.0, 1.0) * (self.height - 1) as f32).max(0.0);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.width - 1), (y0 + 1).min(self.height - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| self.data[y * self.width + x] as f32;
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bottom = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        (top + (bottom - top) * fy) / 255.0
    }
}

fn load_bitmap(path: &str) -> Option<Arc<MaskBitmap>> {
    let file = Path::new(path);
    let dir = std::fs::canonicalize(masks_dir()).ok()?;
    if !std::fs::canonicalize(file).ok()?.starts_with(&dir) {
        return None;
    }
    let bytes = crate::security::read_secure_file(file, 256 << 20).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_luma8();
    if img.width() == 0 || img.height() == 0 {
        return None;
    }
    Some(Arc::new(MaskBitmap { width: img.width() as usize, height: img.height() as usize, data: img.into_raw() }))
}

/// Mask images, loaded once per process (mask files are never rewritten: a new mask
/// gets a new name); a per-thread slot avoids the lock per pixel.
fn bitmap(path: &str) -> Option<Arc<MaskBitmap>> {
    thread_local! {
        static LAST: RefCell<Option<(String, Option<Arc<MaskBitmap>>)>> = const { RefCell::new(None) };
    }
    LAST.with(|last| {
        if let Some((p, m)) = last.borrow().as_ref()
            && p == path
        {
            return m.clone();
        }
        static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<MaskBitmap>>>>> = OnceLock::new();
        let loaded = CACHE
            .get_or_init(Default::default)
            .lock()
            .map(|mut c| c.entry(path.to_string()).or_insert_with(|| load_bitmap(path)).clone())
            .unwrap_or(None);
        *last.borrow_mut() = Some((path.to_string(), loaded.clone()));
        loaded
    })
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AdjustmentLayer {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub invert: bool,
    pub opacity: f32, // 0.0 to 1.0
    pub shape: MaskShape,
    /// Further shapes added to, subtracted from or intersected with `shape`
    pub components: Vec<MaskComponent>,

    // Targeted Adjustments applied through this mask
    pub exposure: f32,   // -5.0 to +5.0 EV
    pub contrast: f32,   // -100.0 to +100.0
    pub highlights: f32, // -100.0 to +100.0
    pub shadows: f32,    // -100.0 to +100.0
    pub whites: f32,     // -100.0 to +100.0
    pub blacks: f32,     // -100.0 to +100.0
    pub clarity: f32,    // -100.0 to +100.0
    pub saturation: f32, // -100.0 to +100.0
    pub tint_r: f32,     // -1.0 to 1.0
    pub tint_g: f32,     // -1.0 to 1.0
    pub tint_b: f32,     // -1.0 to 1.0
}

impl Default for AdjustmentLayer {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "New Layer".to_string(),
            enabled: true,
            invert: false,
            opacity: 1.0,
            shape: MaskShape::LinearGradient {
                start_x: 0.5,
                start_y: 0.0,
                end_x: 0.5,
                end_y: 0.5,
            },
            components: Vec::new(),
            exposure: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            clarity: 0.0,
            saturation: 0.0,
            tint_r: 0.0,
            tint_g: 0.0,
            tint_b: 0.0,
        }
    }
}

/// Evaluates mask weight (0.0 to 1.0) for a given pixel at normalized coordinates (u, v) and RGB luminance
#[inline]
pub fn sample_mask_weight(
    shape: &MaskShape,
    invert: bool,
    opacity: f32,
    u: f32,
    v: f32,
    luma: f32,
) -> f32 {
    let raw_weight = shape_weight(shape, u, v, (luma, luma, luma));
    let w = if invert { 1.0 - raw_weight } else { raw_weight };
    (w * opacity).clamp(0.0, 1.0)
}

/// A layer's mask at one pixel: the main shape combined with each component in
/// order, then the layer's invert and opacity.
#[inline]
pub fn layer_mask_weight(layer: &AdjustmentLayer, u: f32, v: f32, rgb: (f32, f32, f32)) -> f32 {
    let mut w = shape_weight(&layer.shape, u, v, rgb);
    for c in &layer.components {
        let cw = shape_weight(&c.shape, u, v, rgb);
        let cw = if c.invert { 1.0 - cw } else { cw };
        w = match c.mode {
            MaskMode::Add => w.max(cw),
            MaskMode::Subtract => w * (1.0 - cw),
            MaskMode::Intersect => w * cw,
        };
    }
    let w = if layer.invert { 1.0 - w } else { w };
    (w * layer.opacity).clamp(0.0, 1.0)
}

/// The weight of one shape (0..1) at normalised frame coordinates for a pixel colour.
#[inline]
pub fn shape_weight(shape: &MaskShape, u: f32, v: f32, rgb: (f32, f32, f32)) -> f32 {
    let luma = 0.2126 * rgb.0 + 0.7152 * rgb.1 + 0.0722 * rgb.2;
    match shape {
        MaskShape::LinearGradient { start_x, start_y, end_x, end_y } => {
            let dx = end_x - start_x;
            let dy = end_y - start_y;
            let len_sq = dx * dx + dy * dy;
            if len_sq <= 1e-6 {
                1.0
            } else {
                let px = u - start_x;
                let py = v - start_y;
                let proj = (px * dx + py * dy) / len_sq;
                // Raised-cosine / smoothstep transition along the vector
                let clamped = proj.clamp(0.0, 1.0);
                clamped * clamped * (3.0 - 2.0 * clamped)
            }
        }
        MaskShape::RadialGradient { center_x, center_y, radius_x, radius_y, feather } => {
            let rx = radius_x.max(1e-4);
            let ry = radius_y.max(1e-4);
            let f = feather.clamp(0.01, 1.0);

            let nx = (u - center_x) / rx;
            let ny = (v - center_y) / ry;
            let dist = (nx * nx + ny * ny).sqrt();

            if dist >= 1.0 {
                0.0
            } else if dist <= (1.0 - f) {
                1.0
            } else {
                // Smooth falloff in the feathered margin
                let t = (1.0 - dist) / f;
                t * t * (3.0 - 2.0 * t)
            }
        }
        MaskShape::LumaRange { min_luma, max_luma, falloff } => {
            let f = falloff.clamp(0.001, 0.5);
            let min_in = (min_luma - f).max(0.0);
            let max_in = (max_luma + f).min(1.0);

            if luma < min_in || luma > max_in {
                0.0
            } else if luma >= *min_luma && luma <= *max_luma {
                1.0
            } else if luma < *min_luma {
                let t = (luma - min_in) / f;
                t * t * (3.0 - 2.0 * t)
            } else {
                let t = (max_in - luma) / f;
                t * t * (3.0 - 2.0 * t)
            }
        }
        MaskShape::Bitmap { path } => bitmap(path).map(|m| m.sample(u, v)).unwrap_or(0.0),
        MaskShape::Brush { strokes, aspect, cache } => {
            if strokes.is_empty() {
                0.0
            } else {
                cache.0.get_or_init(|| Arc::new(rasterize_brush(strokes, *aspect))).sample(u, v)
            }
        }
        MaskShape::ColorRange { r, g, b, range, feather } => {
            let t = oklab(*r, *g, *b);
            let p = oklab(rgb.0, rgb.1, rgb.2);
            // Lightness counts half as much as hue and chroma
            let d = (0.5 * (p[0] - t[0]).powi(2) + (p[1] - t[1]).powi(2) + (p[2] - t[2]).powi(2)).sqrt();
            let inner = range.clamp(0.0, 1.0) * 0.3;
            let soft = feather.clamp(0.0, 1.0) * 0.3;
            if d <= inner {
                1.0
            } else if soft <= 1e-6 {
                0.0
            } else {
                smoothstep(1.0 - (d - inner) / soft)
            }
        }
    }
}

static DISPLAY: std::sync::LazyLock<crate::pipeline::transfer::Transfer> =
    std::sync::LazyLock::new(crate::pipeline::transfer::Transfer::bt709);

/// Middle grey (18 % reflectance) on the display-encoded scale the layers work on.
const MID_GREY: f32 = 0.41;

/// Applies active adjustment layers to a display-encoded RGB pixel (0.0..1.0). The
/// clarity field is not applied (it needs the neighbourhood, which this per-pixel
/// stage has not got) and is not offered in the interface.
#[inline]
pub fn apply_layers_pixel(
    mut r: f32,
    mut g: f32,
    mut b: f32,
    layers: &[AdjustmentLayer],
    u: f32,
    v: f32,
) -> (f32, f32, f32) {
    if layers.is_empty() {
        return (r, g, b);
    }

    for layer in layers {
        if !layer.enabled || layer.opacity <= 1e-4 {
            continue;
        }

        let weight = layer_mask_weight(layer, u, v, (r, g, b));
        if weight <= 1e-4 {
            continue;
        }

        // Layer exposure acts on light: the values here are display-encoded, so they
        // are decoded, scaled by 2^EV and encoded again
        let (mut lr, mut lg, mut lb) = if layer.exposure.abs() > 1e-4 {
            let t = &*DISPLAY;
            let exp_mul = 2.0f32.powf(layer.exposure * weight);
            (t.encode(t.decode(r) * exp_mul), t.encode(t.decode(g) * exp_mul), t.encode(t.decode(b) * exp_mul))
        } else {
            (r, g, b)
        };

        // Layer highlights, shadows, whites and blacks: the global tone controls'
        // luminance zones, applied as a shift that keeps the colour
        if layer.highlights != 0.0 || layer.shadows != 0.0 || layer.whites != 0.0 || layer.blacks != 0.0 {
            let l = (0.2126 * lr + 0.7152 * lg + 0.0722 * lb).max(0.0);
            let mut shift = 0.0;
            let h = ((l - 0.35).max(0.0) / 0.65).min(1.0);
            shift += layer.highlights / 100.0 * h * h * (3.0 - 2.0 * h) * 0.35;
            let fall = (1.0 - (l / 0.60).min(1.0)).powi(2);
            shift += layer.shadows / 100.0 * fall * (l / (l + 0.03)) * 0.35;
            shift += layer.whites / 100.0 * (((l - 0.60).max(0.0) / 0.40).min(1.0)).powi(2) * 0.25;
            shift += layer.blacks / 100.0 * (((0.30 - l).max(0.0) / 0.30).min(1.0)).powi(2) * 0.20;
            lr += shift * weight;
            lg += shift * weight;
            lb += shift * weight;
        }

        // Layer contrast around middle grey (0.18 linear is about 0.41 encoded)
        if layer.contrast.abs() > 1e-3 {
            let c_factor = 1.0 + (layer.contrast / 100.0) * weight;
            lr = MID_GREY + (lr - MID_GREY) * c_factor;
            lg = MID_GREY + (lg - MID_GREY) * c_factor;
            lb = MID_GREY + (lb - MID_GREY) * c_factor;
        }

        // Layer saturation
        if layer.saturation.abs() > 1e-3 {
            let sat_mul = 1.0 + (layer.saturation / 100.0) * weight;
            let l = 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
            lr = l + (lr - l) * sat_mul;
            lg = l + (lg - l) * sat_mul;
            lb = l + (lb - l) * sat_mul;
        }

        // Layer tint
        if layer.tint_r.abs() > 1e-4 || layer.tint_g.abs() > 1e-4 || layer.tint_b.abs() > 1e-4 {
            lr += layer.tint_r * weight * 0.2;
            lg += layer.tint_g * weight * 0.2;
            lb += layer.tint_b * weight * 0.2;
        }

        r = lr.max(0.0);
        g = lg.max(0.0);
        b = lb.max(0.0);
    }

    (r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_mask_is_sampled_bilinearly() {
        // 2x2: black left column, white right column
        let m = MaskBitmap { width: 2, height: 2, data: vec![0, 255, 0, 255] };
        assert!(m.sample(0.0, 0.0).abs() < 1e-6);
        assert!((m.sample(1.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((m.sample(0.5, 0.3) - 0.5).abs() < 1e-3);
        // Coordinates outside the frame clamp to the edge
        assert!((m.sample(2.0, -1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bitmap_outside_masks_dir_is_ignored() {
        let shape = MaskShape::Bitmap { path: "/etc/hostname".into() };
        assert_eq!(sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.5), 0.0);
    }

    #[test]
    fn test_linear_gradient_interpolation() {
        let shape = MaskShape::LinearGradient {
            start_x: 0.0,
            start_y: 0.0,
            end_x: 1.0,
            end_y: 0.0,
        };

        let w_start = sample_mask_weight(&shape, false, 1.0, 0.0, 0.5, 0.5);
        let w_mid = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.5);
        let w_end = sample_mask_weight(&shape, false, 1.0, 1.0, 0.5, 0.5);

        assert!((w_start - 0.0).abs() < 1e-3);
        assert!((w_mid - 0.5).abs() < 1e-2);
        assert!((w_end - 1.0).abs() < 1e-3);
    }

    #[test]
    fn test_radial_gradient_bounds() {
        let shape = MaskShape::RadialGradient {
            center_x: 0.5,
            center_y: 0.5,
            radius_x: 0.2,
            radius_y: 0.2,
            feather: 0.5,
        };

        let w_center = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.5);
        let w_outside = sample_mask_weight(&shape, false, 1.0, 0.9, 0.9, 0.5);

        assert!((w_center - 1.0).abs() < 1e-3);
        assert_eq!(w_outside, 0.0);
    }

    /// +1 EV on a layer doubles the light, which is 2^0.45 on encoded values.
    #[test]
    fn layer_exposure_is_one_stop_per_ev() {
        let layer = AdjustmentLayer {
            shape: MaskShape::LumaRange { min_luma: 0.0, max_luma: 1.0, falloff: 0.01 },
            exposure: 1.0,
            ..Default::default()
        };
        let t = crate::pipeline::transfer::Transfer::bt709();
        let enc = t.encode(0.1);
        let (r, _, _) = apply_layers_pixel(enc, enc, enc, &[layer], 0.5, 0.5);
        let linear = t.decode(r);
        assert!((linear / 0.1 - 2.0).abs() < 0.08, "{linear}");
    }

    #[test]
    fn layer_tone_zones_apply() {
        let layer = AdjustmentLayer {
            shape: MaskShape::LumaRange { min_luma: 0.0, max_luma: 1.0, falloff: 0.01 },
            shadows: 60.0,
            ..Default::default()
        };
        let (r, g, b) = apply_layers_pixel(0.1, 0.1, 0.1, &[layer], 0.5, 0.5);
        assert!(r > 0.12 && (r - g).abs() < 1e-6 && (g - b).abs() < 1e-6);
    }

    fn brush(strokes: Vec<BrushStroke>) -> MaskShape {
        MaskShape::Brush { strokes, aspect: 1.0, cache: BrushCache::default() }
    }

    #[test]
    fn brush_paints_and_erases() {
        let paint = BrushStroke { points: vec![[0.2, 0.5], [0.8, 0.5]], size: 0.1, feather: 0.0, ..Default::default() };
        let shape = brush(vec![paint.clone()]);
        let rgb = (0.5, 0.5, 0.5);
        assert!(shape_weight(&shape, 0.5, 0.5, rgb) > 0.99);
        assert!(shape_weight(&shape, 0.2, 0.55, rgb) > 0.99);
        assert!(shape_weight(&shape, 0.5, 0.8, rgb) < 0.01);
        let erase = BrushStroke { points: vec![[0.5, 0.5]], size: 0.05, feather: 0.0, erase: true, ..Default::default() };
        let shape = brush(vec![paint.clone(), erase]);
        assert!(shape_weight(&shape, 0.5, 0.5, rgb) < 0.01);
        assert!(shape_weight(&shape, 0.3, 0.5, rgb) > 0.99);
        // Half flow paints half; a soft edge falls off towards the radius
        let shape = brush(vec![BrushStroke { flow: 0.5, ..paint.clone() }]);
        assert!((shape_weight(&shape, 0.5, 0.5, rgb) - 0.5).abs() < 0.01);
        let shape = brush(vec![BrushStroke { feather: 1.0, ..paint }]);
        let edge = shape_weight(&shape, 0.5, 0.57, rgb);
        assert!(edge > 0.0 && edge < 0.5, "{edge}");
    }

    #[test]
    fn brush_survives_a_json_round_trip() {
        let layer = AdjustmentLayer { shape: brush(vec![BrushStroke { points: vec![[0.5, 0.5]], ..Default::default() }]), ..Default::default() };
        let json = serde_json::to_string(&layer).unwrap();
        assert!(!json.contains("cache"));
        let back: AdjustmentLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, layer);
    }

    #[test]
    fn colour_range_selects_near_colours() {
        let shape = MaskShape::ColorRange { r: 0.2, g: 0.4, b: 0.8, range: 0.2, feather: 0.2 };
        assert!(shape_weight(&shape, 0.5, 0.5, (0.2, 0.4, 0.8)) > 0.99);
        assert!(shape_weight(&shape, 0.5, 0.5, (0.22, 0.42, 0.78)) > 0.99);
        assert!(shape_weight(&shape, 0.5, 0.5, (0.8, 0.4, 0.2)) < 0.01);
    }

    #[test]
    fn components_add_subtract_and_intersect() {
        let left = MaskShape::LinearGradient { start_x: 0.6, start_y: 0.0, end_x: 0.4, end_y: 0.0 };
        let centre = MaskShape::RadialGradient { center_x: 0.5, center_y: 0.5, radius_x: 0.2, radius_y: 0.2, feather: 0.01 };
        let rgb = (0.5, 0.5, 0.5);
        let mut layer = AdjustmentLayer { shape: left, ..Default::default() };
        assert!(layer_mask_weight(&layer, 0.1, 0.5, rgb) > 0.99);
        assert!(layer_mask_weight(&layer, 0.9, 0.5, rgb) < 0.01);
        layer.components = vec![MaskComponent { mode: MaskMode::Add, invert: false, shape: centre.clone() }];
        assert!(layer_mask_weight(&layer, 0.65, 0.5, rgb) > 0.99);
        layer.components[0].mode = MaskMode::Subtract;
        assert!(layer_mask_weight(&layer, 0.35, 0.5, rgb) < 0.01);
        assert!(layer_mask_weight(&layer, 0.1, 0.5, rgb) > 0.99);
        layer.components[0].mode = MaskMode::Intersect;
        assert!(layer_mask_weight(&layer, 0.35, 0.5, rgb) > 0.99);
        assert!(layer_mask_weight(&layer, 0.1, 0.5, rgb) < 0.01);
        // The layer's invert applies to the combined mask
        layer.invert = true;
        assert!(layer_mask_weight(&layer, 0.1, 0.5, rgb) > 0.99);
    }

    #[test]
    fn test_luma_range_mask() {
        let shape = MaskShape::LumaRange {
            min_luma: 0.6,
            max_luma: 1.0,
            falloff: 0.1,
        };

        let w_shadow = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.2);
        let w_highlight = sample_mask_weight(&shape, false, 1.0, 0.5, 0.5, 0.8);

        assert_eq!(w_shadow, 0.0);
        assert!((w_highlight - 1.0).abs() < 1e-3);
    }
}
