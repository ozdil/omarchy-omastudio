//! Lens corrections for medium format: lens cast calibration (flat field, the way
//! Phocus and Capture One correct technical camera and shift lenses), lens profiles
//! from the Lensfun database installed on the system, and fitting a vignetting
//! profile from a flat field so new lenses (XCD, HC) can be profiled by their owners.
//!
//! Everything runs on linear RGB right after demosaicing, before white balance and
//! tone, where light falloff is a plain multiplication.
//!
//! Lensfun data is read at run time from the user's system (`lensfun` package) and
//! from `~/.local/share/lensfun`; none of it is bundled here.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Grid size of a flat field gain map; lens cast is smooth, so a coarse grid that is
/// interpolated bilinearly is enough and keeps the file small.
const GRID_W: usize = 48;
const GRID_H: usize = 36;

// ---------------------------------------------------------------------------------
// Lens profiles (Lensfun XML)

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "model", rename_all = "snake_case")]
pub enum Distortion {
    /// r_d = r_u (1 - k1 + k1 r_u²)
    Poly3 { k1: f32 },
    /// r_d = r_u (1 + k1 r_u² + k2 r_u⁴)
    Poly5 { k1: f32, k2: f32 },
    /// r_d = r_u (a r_u³ + b r_u² + c r_u + 1 - a - b - c)
    Ptlens { a: f32, b: f32, c: f32 },
}

impl Distortion {
    /// Distorted radius for an undistorted (corrected) radius.
    pub fn distorted(&self, ru: f32) -> f32 {
        match *self {
            Self::Poly3 { k1 } => ru * (1.0 - k1 + k1 * ru * ru),
            Self::Poly5 { k1, k2 } => {
                let r2 = ru * ru;
                ru * (1.0 + k1 * r2 + k2 * r2 * r2)
            }
            Self::Ptlens { a, b, c } => ru * (a * ru * ru * ru + b * ru * ru + c * ru + 1.0 - a - b - c),
        }
    }

    fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let m = |x: f32, y: f32| x + (y - x) * t;
        match (*a, *b) {
            (Self::Poly3 { k1: x }, Self::Poly3 { k1: y }) => Self::Poly3 { k1: m(x, y) },
            (Self::Poly5 { k1: x1, k2: x2 }, Self::Poly5 { k1: y1, k2: y2 }) => Self::Poly5 { k1: m(x1, y1), k2: m(x2, y2) },
            (Self::Ptlens { a: xa, b: xb, c: xc }, Self::Ptlens { a: ya, b: yb, c: yc }) => {
                Self::Ptlens { a: m(xa, ya), b: m(xb, yb), c: m(xc, yc) }
            }
            _ => if t < 0.5 { *a } else { *b },
        }
    }
}

/// Lateral chromatic aberration: red and blue radius scale relative to green.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Tca {
    pub kr: f32,
    pub kb: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LensProfile {
    pub maker: String,
    pub model: String,
    pub crop_factor: f32,
    /// (focal length, model)
    pub distortion: Vec<(f32, Distortion)>,
    pub tca: Vec<(f32, Tca)>,
    /// (focal length, aperture, distance, [k1, k2, k3]) of the PA model.
    pub vignetting: Vec<(f32, f32, f32, [f32; 3])>,
}

/// Corrections for one shot (a profile resolved at its focal length and aperture).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Correction {
    pub distortion: Option<Distortion>,
    pub tca: Option<Tca>,
    pub vignetting: Option<[f32; 3]>,
}

impl Correction {
    pub fn is_empty(&self) -> bool {
        self.distortion.is_none() && self.tca.is_none() && self.vignetting.is_none()
    }
}

impl LensProfile {
    /// Parameters at `focal` mm and f/`aperture`, interpolated between calibrations.
    pub fn at(&self, focal: f32, aperture: f32) -> Correction {
        let distortion = interpolate(&self.distortion, focal, Distortion::lerp);
        let tca = interpolate(&self.tca, focal, |a, b, t| Tca { kr: a.kr + (b.kr - a.kr) * t, kb: a.kb + (b.kb - a.kb) * t });
        // Vignetting: nearest focal length, then nearest aperture (in stops), farthest distance
        let vignetting = self
            .vignetting
            .iter()
            .min_by(|a, b| {
                let score = |v: &(f32, f32, f32, [f32; 3])| {
                    (v.0 - focal).abs() / focal.max(1.0) * 10.0 + (v.1.max(0.5).log2() - aperture.max(0.5).log2()).abs() - v.2.min(1000.0) * 1e-4
                };
                score(a).total_cmp(&score(b))
            })
            .map(|v| v.3);
        Correction { distortion, tca, vignetting }
    }
}

fn interpolate<T: Copy>(items: &[(f32, T)], focal: f32, lerp: impl Fn(&T, &T, f32) -> T) -> Option<T> {
    let mut v: Vec<&(f32, T)> = items.iter().collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = v.first()?;
    if focal <= first.0 || v.len() == 1 {
        return Some(first.1);
    }
    for w in v.windows(2) {
        if focal <= w[1].0 {
            let t = (focal - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
            return Some(lerp(&w[0].1, &w[1].1, t));
        }
    }
    v.last().map(|l| l.1)
}

/// Folders Lensfun databases are read from, system first, user last (user wins).
pub fn lensfun_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let mut dirs = vec![
        PathBuf::from("/usr/share/lensfun/version_1"),
        PathBuf::from("/usr/share/lensfun"),
        PathBuf::from("/usr/local/share/lensfun/version_1"),
        PathBuf::from(&home).join(".local/share/lensfun/updates/version_1"),
        PathBuf::from(&home).join(".local/share/lensfun"),
    ];
    dirs.push(PathBuf::from(home).join(".local/share/omastudio/lenses"));
    dirs
}

/// All lens profiles in the Lensfun XML files found in `dirs`.
pub fn load_profiles(dirs: &[PathBuf]) -> Vec<LensProfile> {
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "xml")).collect();
        files.sort();
        for f in files {
            if let Ok(xml) = crate::security::read_secure_file(&f, 8 * 1024 * 1024)
                && let Ok(text) = String::from_utf8(xml)
            {
                out.extend(parse_lensfun(&text));
            }
        }
    }
    out
}

/// Normalises a lens name for matching: lower case, no punctuation or "mm".
fn normalise(s: &str) -> String {
    s.to_lowercase()
        .replace("mm", " ")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The profile whose model best matches the lens name the camera recorded.
pub fn find_profile<'a>(profiles: &'a [LensProfile], lens: &str) -> Option<&'a LensProfile> {
    let want = normalise(lens);
    if want.is_empty() {
        return None;
    }
    let want_words: Vec<&str> = want.split(' ').collect();
    profiles
        .iter()
        .rev() // later (user) files override system ones
        .filter_map(|p| {
            let have = normalise(&p.model);
            if have == want {
                return Some((p, 1000));
            }
            let words: Vec<&str> = have.split(' ').collect();
            // Every word of the profile must appear in the recorded name
            let all = words.iter().all(|w| want_words.contains(w));
            all.then_some((p, words.len() as i32 * 10 - (want_words.len() as i32 - words.len() as i32).abs()))
        })
        .max_by_key(|(_, s)| *s)
        .map(|(p, _)| p)
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!("{}=\"", name);
    let start = tag.find(&format!(" {}", pat)).map(|i| i + 1 + pat.len())?;
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

fn attr_f(tag: &str, name: &str) -> Option<f32> {
    attr(tag, name)?.trim().parse().ok()
}

fn element_text(block: &str, name: &str) -> Option<String> {
    // First <name> or <name lang="..."> without a lang attribute wins
    let mut rest = block;
    let mut first = None;
    while let Some(i) = rest.find(&format!("<{}", name)) {
        let after = &rest[i + 1 + name.len()..];
        let close = after.find('>')?;
        let tag = &after[..close];
        let body = &after[close + 1..];
        let end = body.find(&format!("</{}>", name))?;
        let text = body[..end].trim().to_string();
        if !tag.contains("lang=") {
            return Some(text);
        }
        first.get_or_insert(text);
        rest = &body[end..];
    }
    first
}

/// Parses the `<lens>` entries of a Lensfun XML database.
pub fn parse_lensfun(xml: &str) -> Vec<LensProfile> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<lens>") {
        let body = &rest[i + 6..];
        let Some(end) = body.find("</lens>") else { break };
        let block = &body[..end];
        rest = &body[end..];

        let mut p = LensProfile {
            maker: element_text(block, "maker").unwrap_or_default(),
            model: element_text(block, "model").unwrap_or_default(),
            crop_factor: element_text(block, "cropfactor").and_then(|c| c.parse().ok()).unwrap_or(1.0),
            distortion: Vec::new(),
            tca: Vec::new(),
            vignetting: Vec::new(),
        };
        for tag in block.split('<').filter(|t| t.starts_with("distortion") || t.starts_with("tca") || t.starts_with("vignetting")) {
            let focal = attr_f(tag, "focal").unwrap_or(0.0);
            let model = attr(tag, "model").unwrap_or_default();
            let g = |n: &str| attr_f(tag, n).unwrap_or(0.0);
            if tag.starts_with("distortion") {
                let d = match model.as_str() {
                    "poly3" => Distortion::Poly3 { k1: g("k1") },
                    "poly5" => Distortion::Poly5 { k1: g("k1"), k2: g("k2") },
                    "ptlens" => Distortion::Ptlens { a: g("a"), b: g("b"), c: g("c") },
                    _ => continue,
                };
                p.distortion.push((focal, d));
            } else if tag.starts_with("tca") {
                match model.as_str() {
                    "linear" => p.tca.push((focal, Tca { kr: attr_f(tag, "kr").unwrap_or(1.0), kb: attr_f(tag, "kb").unwrap_or(1.0) })),
                    // poly3 at the edge of the frame, folded to a linear scale
                    "poly3" => {
                        let at1 = |v: &str, c: &str, b: &str| attr_f(tag, v).unwrap_or(1.0) + g(c) + g(b);
                        p.tca.push((focal, Tca { kr: at1("vr", "cr", "br"), kb: at1("vb", "cb", "bb") }))
                    }
                    _ => {}
                }
            } else if model == "pa" {
                p.vignetting.push((focal, g("aperture"), attr_f(tag, "distance").unwrap_or(1000.0), [g("k1"), g("k2"), g("k3")]));
            }
        }
        if !p.model.is_empty() {
            out.push(p);
        }
    }
    out
}

// ---------------------------------------------------------------------------------
// Flat field (lens cast calibration)

/// Per-channel light falloff of one lens/back/shift combination, measured from a
/// shot of an evenly lit white diffuser. Stored in sensor orientation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlatField {
    pub grid_w: usize,
    pub grid_h: usize,
    /// Relative gain per grid cell, RGB interleaved, 1.0 at the brightest cell.
    pub gain: Vec<f32>,
    /// LibRaw flip of the flat field shot; maps are stored unrotated.
    pub flip: i32,
    /// What it was made from, for the user.
    pub source: String,
}

impl FlatField {
    /// Builds a flat field from a developed (linear, rotated by `flip`) image.
    pub fn from_linear(rgb: &[u16], width: usize, height: usize, flip: i32, source: &str) -> Option<Self> {
        if width < GRID_W * 4 || height < GRID_H * 4 || rgb.len() < width * height * 3 {
            return None;
        }
        // Unrotate to sensor orientation, then average each cell
        let (sw, sh) = if matches!(flip, 5 | 6) { (height, width) } else { (width, height) };
        let mut sums = vec![0f64; GRID_W * GRID_H * 3];
        let mut counts = vec![0u32; GRID_W * GRID_H];
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = to_sensor(x, y, width, height, flip);
                let (gx, gy) = (sx * GRID_W / sw, sy * GRID_H / sh);
                let cell = gy * GRID_W + gx;
                let p = (y * width + x) * 3;
                for c in 0..3 {
                    sums[cell * 3 + c] += rgb[p + c] as f64;
                }
                counts[cell] += 1;
            }
        }
        let mut gain: Vec<f32> = sums.iter().enumerate().map(|(i, s)| (*s / counts[i / 3].max(1) as f64) as f32).collect();
        for c in 0..3 {
            let max = (0..GRID_W * GRID_H).map(|i| gain[i * 3 + c]).fold(0.0f32, f32::max);
            if max <= 0.0 {
                return None;
            }
            for i in 0..GRID_W * GRID_H {
                gain[i * 3 + c] = (gain[i * 3 + c] / max).max(0.05);
            }
        }
        Some(Self { grid_w: GRID_W, grid_h: GRID_H, gain, flip, source: source.to_string() })
    }

    /// Gain (RGB) at sensor position (u, v) in 0..1, bilinear between cell centres.
    fn sample(&self, u: f32, v: f32) -> [f32; 3] {
        let fx = (u * self.grid_w as f32 - 0.5).clamp(0.0, (self.grid_w - 1) as f32);
        let fy = (v * self.grid_h as f32 - 0.5).clamp(0.0, (self.grid_h - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(self.grid_w - 1), (y0 + 1).min(self.grid_h - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        std::array::from_fn(|c| {
            let g = |x: usize, y: usize| self.gain[(y * self.grid_w + x) * 3 + c];
            let top = g(x0, y0) + (g(x1, y0) - g(x0, y0)) * tx;
            let bottom = g(x0, y1) + (g(x1, y1) - g(x0, y1)) * tx;
            top + (bottom - top) * ty
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| format!("Cannot write {}: {}", path.display(), e))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let data = crate::security::read_secure_file(path, 4 * 1024 * 1024).map_err(|e| e.to_string())?;
        let f: Self = serde_json::from_slice(&data).map_err(|e| format!("Not a flat field file: {}", e))?;
        if f.grid_w == 0 || f.grid_h == 0 || f.gain.len() != f.grid_w * f.grid_h * 3 {
            return Err("Flat field grid is inconsistent".into());
        }
        Ok(f)
    }

    /// Radial PA vignetting model (k1, k2, k3) fitted to the green channel, in the
    /// Lensfun convention (r = 1 at the corner), for writing a Lensfun profile.
    pub fn fit_vignetting(&self) -> [f32; 3] {
        // Least squares on 1/gain - 1 = k1 r² + k2 r⁴ + k3 r⁶
        let mut ata = [[0f64; 3]; 3];
        let mut atb = [0f64; 3];
        let aspect = self.grid_w as f64 / self.grid_h as f64;
        let half_diag = (aspect * aspect + 1.0).sqrt();
        for gy in 0..self.grid_h {
            for gx in 0..self.grid_w {
                let x = ((gx as f64 + 0.5) / self.grid_w as f64 - 0.5) * 2.0 * aspect;
                let y = ((gy as f64 + 0.5) / self.grid_h as f64 - 0.5) * 2.0;
                let r2 = (x * x + y * y) / (half_diag * half_diag);
                let basis = [r2, r2 * r2, r2 * r2 * r2];
                let g = self.gain[(gy * self.grid_w + gx) * 3 + 1] as f64;
                // The model is relative to the centre, not to the brightest cell
                let target = g - 1.0;
                for i in 0..3 {
                    atb[i] += basis[i] * target;
                    for j in 0..3 {
                        ata[i][j] += basis[i] * basis[j];
                    }
                }
            }
        }
        solve3(ata, atb).map(|k| [k[0] as f32, k[1] as f32, k[2] as f32]).unwrap_or([0.0; 3])
    }
}

/// Lensfun XML for a lens profiled from a flat field, ready for ~/.local/share/lensfun.
pub fn lensfun_xml(maker: &str, model: &str, mount: &str, crop_factor: f32, focal: f32, aperture: f32, k: [f32; 3]) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
    format!(
        "<lensdatabase version=\"1\">\n    <lens>\n        <maker>{}</maker>\n        <model>{}</model>\n        <mount>{}</mount>\n        <cropfactor>{}</cropfactor>\n        <calibration>\n            <vignetting model=\"pa\" focal=\"{}\" aperture=\"{}\" distance=\"1000\" k1=\"{:.5}\" k2=\"{:.5}\" k3=\"{:.5}\"/>\n        </calibration>\n    </lens>\n</lensdatabase>\n",
        esc(maker),
        esc(model),
        esc(mount),
        crop_factor,
        focal,
        aperture,
        k[0],
        k[1],
        k[2]
    )
}

fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(&a);
    if d.abs() < 1e-12 {
        return None;
    }
    Some(std::array::from_fn(|i| {
        let mut m = a;
        for r in 0..3 {
            m[r][i] = b[r];
        }
        det(&m) / d
    }))
}

/// Pixel (x, y) of an image rotated by LibRaw `flip` -> sensor position.
fn to_sensor(x: usize, y: usize, width: usize, height: usize, flip: i32) -> (usize, usize) {
    match flip {
        3 => (width - 1 - x, height - 1 - y),
        // 90° CCW: output (x, y) came from sensor (sw-1-y, x) with sw = height
        5 => (height - 1 - y, x),
        // 90° CW
        6 => (y, width - 1 - x),
        _ => (x, y),
    }
}

// ---------------------------------------------------------------------------------
// Applying corrections

/// Applies flat field, vignetting, lateral CA and distortion to linear 16-bit RGB
/// (`flip` is the rotation the image was developed with).
pub fn apply(rgb: &mut [u16], width: usize, height: usize, flip: i32, flat: Option<&FlatField>, c: &Correction) {
    if flat.is_none() && c.is_empty() {
        return;
    }
    let (cx, cy) = ((width as f32 - 1.0) / 2.0, (height as f32 - 1.0) / 2.0);
    let short = (width.min(height) as f32 / 2.0).max(1.0);
    let diag = (cx * cx + cy * cy).sqrt().max(1.0);
    let (sw, sh) = if matches!(flip, 5 | 6) { (height as f32, width as f32) } else { (width as f32, height as f32) };

    // 1. Falloff: flat field gain map, else the profile's vignetting model
    rgb.par_chunks_mut(width * 3).enumerate().for_each(|(y, row)| {
        for x in 0..width {
            let mut gain = [1f32; 3];
            if let Some(f) = flat {
                let (sx, sy) = to_sensor(x, y, width, height, flip);
                gain = f.sample((sx as f32 + 0.5) / sw, (sy as f32 + 0.5) / sh);
            } else if let Some([k1, k2, k3]) = c.vignetting {
                let (dx, dy) = ((x as f32 - cx) / diag, (y as f32 - cy) / diag);
                let r2 = dx * dx + dy * dy;
                let g = (1.0 + k1 * r2 + k2 * r2 * r2 + k3 * r2 * r2 * r2).max(0.05);
                gain = [g; 3];
            }
            for ch in 0..3 {
                let v = row[x * 3 + ch] as f32 / gain[ch];
                row[x * 3 + ch] = v.round().clamp(0.0, 65535.0) as u16;
            }
        }
    });

    // 2. Geometry: each output pixel samples the source where the lens put it
    if c.distortion.is_none() && c.tca.is_none() {
        return;
    }
    let src = rgb.to_vec();
    let dist = c.distortion;
    let tca = c.tca.unwrap_or(Tca { kr: 1.0, kb: 1.0 });
    rgb.par_chunks_mut(width * 3).enumerate().for_each(|(y, row)| {
        for x in 0..width {
            let (nx, ny) = ((x as f32 - cx) / short, (y as f32 - cy) / short);
            let ru = (nx * nx + ny * ny).sqrt();
            let scale = match dist {
                Some(d) if ru > 1e-6 => d.distorted(ru) / ru,
                _ => 1.0,
            };
            for (ch, k) in [(0usize, tca.kr), (1, 1.0), (2, tca.kb)] {
                let sx = cx + nx * scale * k * short;
                let sy = cy + ny * scale * k * short;
                row[x * 3 + ch] = bilinear(&src, width, height, sx, sy, ch);
            }
        }
    });
}

fn bilinear(src: &[u16], width: usize, height: usize, x: f32, y: f32, ch: usize) -> u16 {
    if x < 0.0 || y < 0.0 || x > (width - 1) as f32 || y > (height - 1) as f32 {
        return 0;
    }
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let p = |xx: usize, yy: usize| src[(yy * width + xx) * 3 + ch] as f32;
    let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * tx;
    let bottom = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * tx;
    (top + (bottom - top) * ty).round().clamp(0.0, 65535.0) as u16
}

/// Flat field file kept next to a raw: `<raw>.lcc.json`, or `lcc.json` in its folder
/// (one calibration for a whole shoot with the same lens and shift).
pub fn flat_field_for(raw: &Path) -> Option<FlatField> {
    let own = PathBuf::from(format!("{}.lcc.json", raw.display()));
    let folder = raw.parent().map(|p| p.join("lcc.json"));
    [Some(own), folder].into_iter().flatten().find(|p| p.is_file()).and_then(|p| FlatField::load(&p).ok())
}

static SUSPENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turns automatic correction off for this process (calibration shots must be read
/// as they are).
pub fn suspend_corrections() {
    SUSPENDED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Lens profiles from the Lensfun folders, read once per process.
pub fn profiles() -> &'static [LensProfile] {
    static PROFILES: std::sync::OnceLock<Vec<LensProfile>> = std::sync::OnceLock::new();
    PROFILES.get_or_init(|| load_profiles(&lensfun_dirs()))
}

/// Corrects a freshly developed medium format image: the shoot's flat field when
/// there is one, otherwise the lens profile matching the recorded lens. Other cameras
/// are left alone (their makers bake corrections into the JPEG look the app already
/// matches). `OMASTUDIO_LENS_CORRECTION=off` turns it off.
pub fn correct_developed(raw: &Path, lens: &str, focal: f32, aperture: f32, rgb: &mut [u16], width: usize, height: usize, flip: i32) -> bool {
    if SUSPENDED.load(std::sync::atomic::Ordering::Relaxed) || std::env::var("OMASTUDIO_LENS_CORRECTION").is_ok_and(|v| v == "off" || v == "0") {
        return false;
    }
    if !super::is_medium_format(raw) {
        return false;
    }
    let flat = flat_field_for(raw);
    let correction = find_profile(profiles(), lens).map(|p| p.at(focal, aperture)).unwrap_or_default();
    // A flat field already holds the lens's falloff and colour cast
    let correction = if flat.is_some() { Correction { vignetting: None, ..correction } } else { correction };
    if flat.is_none() && correction.is_empty() {
        return false;
    }
    apply(rgb, width, height, flip, flat.as_ref(), &correction);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<lensdatabase version="1">
    <lens>
        <maker>Schneider-Kreuznach</maker>
        <model>Schneider LS 80mm f/2.8</model>
        <model lang="de">Schneider LS 80 mm</model>
        <mount>Phase One 645</mount>
        <cropfactor>0.64</cropfactor>
        <calibration>
            <distortion model="ptlens" focal="80" a="0.01" b="-0.02" c="0.005"/>
            <tca model="linear" focal="80" kr="1.0002" kb="0.9998"/>
            <vignetting model="pa" focal="80" aperture="2.8" distance="1000" k1="-0.5" k2="0.1" k3="-0.05"/>
            <vignetting model="pa" focal="80" aperture="8" distance="1000" k1="-0.2" k2="0.05" k3="0"/>
        </calibration>
    </lens>
    <lens>
        <maker>Pentax</maker>
        <model>smc PENTAX-D FA 645 55mm F2.8 AL [IF] SDM AW</model>
        <calibration><distortion model="poly3" focal="55" k1="-0.01"/></calibration>
    </lens>
</lensdatabase>"#;

    #[test]
    fn parses_lensfun_entries() {
        let p = parse_lensfun(XML);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].model, "Schneider LS 80mm f/2.8");
        assert_eq!(p[0].crop_factor, 0.64);
        assert_eq!(p[0].distortion, vec![(80.0, Distortion::Ptlens { a: 0.01, b: -0.02, c: 0.005 })]);
        assert_eq!(p[0].vignetting.len(), 2);
        let c = p[0].at(80.0, 7.1);
        assert_eq!(c.vignetting, Some([-0.2, 0.05, 0.0]));
        assert_eq!(c.tca, Some(Tca { kr: 1.0002, kb: 0.9998 }));
    }

    #[test]
    fn matches_recorded_lens_names() {
        let p = parse_lensfun(XML);
        assert_eq!(find_profile(&p, "Schneider LS 80mm f/2.8").unwrap().maker, "Schneider-Kreuznach");
        assert_eq!(find_profile(&p, "smc PENTAX-D FA 645 55mm F2.8 AL [IF] SDM AW").unwrap().maker, "Pentax");
        assert!(find_profile(&p, "XCD 38V").is_none());
        assert!(find_profile(&p, "").is_none());
    }

    #[test]
    fn interpolates_between_focal_lengths() {
        let items = vec![(30.0, Distortion::Poly3 { k1: 0.0 }), (60.0, Distortion::Poly3 { k1: 0.03 })];
        assert_eq!(interpolate(&items, 45.0, Distortion::lerp), Some(Distortion::Poly3 { k1: 0.015 }));
        assert_eq!(interpolate(&items, 10.0, Distortion::lerp), Some(Distortion::Poly3 { k1: 0.0 }));
        assert_eq!(interpolate(&items, 90.0, Distortion::lerp), Some(Distortion::Poly3 { k1: 0.03 }));
    }

    fn vignetted(w: usize, h: usize, k: f32) -> Vec<u16> {
        let (cx, cy) = ((w as f32 - 1.0) / 2.0, (h as f32 - 1.0) / 2.0);
        let diag = (cx * cx + cy * cy).sqrt();
        let mut v = vec![0u16; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                let r2 = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)) / (diag * diag);
                let g = 1.0 + k * r2;
                for c in 0..3 {
                    v[(y * w + x) * 3 + c] = (40000.0 * g) as u16;
                }
            }
        }
        v
    }

    #[test]
    fn flat_field_evens_out_falloff() {
        let (w, h) = (480, 360);
        let flat_img = vignetted(w, h, -0.4);
        let flat = FlatField::from_linear(&flat_img, w, h, 0, "test").unwrap();
        let mut img = flat_img.clone();
        apply(&mut img, w, h, 0, Some(&flat), &Correction::default());
        let centre = img[((h / 2) * w + w / 2) * 3 + 1] as f32;
        let corner = img[(5 * w + 5) * 3 + 1] as f32;
        assert!((corner / centre - 1.0).abs() < 0.03, "corner {} centre {}", corner, centre);
        // The fitted model finds the falloff
        let k = flat.fit_vignetting();
        assert!((k[0] + 0.4).abs() < 0.1, "{:?}", k);
        assert!(lensfun_xml("Hasselblad", "XCD 38V", "Hasselblad XCD", 0.79, 38.0, 4.0, k).contains("model=\"pa\""));
    }

    #[test]
    fn flat_field_follows_rotation() {
        for flip in [0, 3, 5, 6] {
            let (w, h) = if matches!(flip, 5 | 6) { (360, 480) } else { (480, 360) };
            let mut img = vec![30000u16; w * h * 3];
            // Brighter left edge in sensor terms
            for y in 0..h {
                for x in 0..w {
                    let (sx, _) = to_sensor(x, y, w, h, flip);
                    if sx < 40 {
                        for c in 0..3 {
                            img[(y * w + x) * 3 + c] = 60000;
                        }
                    }
                }
            }
            let flat = FlatField::from_linear(&img, w, h, flip, "t").unwrap();
            assert!(flat.gain[1] > 0.99 && flat.gain[(flat.grid_w - 1) * 3 + 1] < 0.6, "flip {}", flip);
        }
    }

    #[test]
    fn barrel_correction_keeps_the_frame_edge_and_pulls_mid_field_in() {
        let (w, h) = (200, 150);
        let mut img = vec![0u16; w * h * 3];
        // A bright dot 40 px right of centre
        let (cx, cy) = (99usize, 74usize);
        for c in 0..3 {
            img[(cy * w + cx + 40) * 3 + c] = 60000;
        }
        let c = Correction { distortion: Some(Distortion::Poly3 { k1: -0.1 }), ..Default::default() };
        apply(&mut img, w, h, 0, None, &c);
        let row: Vec<u16> = (0..w).map(|x| img[(cy * w + x) * 3 + 1]).collect();
        let peak = row.iter().enumerate().max_by_key(|(_, v)| **v).unwrap().0;
        // Lensfun's poly3 leaves r = 1 in place and magnifies the centre, so the
        // corrected dot at r ≈ 0.53 lands where ru · (1.1 − 0.1 ru²) = 0.53: ~37 px out
        assert!((cx + 34..=cx + 38).contains(&peak), "peak at {}", peak);
    }
}
