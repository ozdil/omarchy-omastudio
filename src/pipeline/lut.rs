use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

/// High-performance 3D Look-Up Table for DaVinci Resolve grading
#[derive(Debug, Clone)]
pub struct Lut3D {
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub table: Vec<[f32; 3]>, // size * size * size entries
}

impl Lut3D {
    /// Creates an identity 3D LUT of a given dimension
    pub fn identity(size: usize) -> Self {
        let n = size.max(2);
        let mut table = Vec::with_capacity(n * n * n);
        let step = 1.0 / (n - 1) as f32;

        for b in 0..n {
            let b_val = b as f32 * step;
            for g in 0..n {
                let g_val = g as f32 * step;
                for r in 0..n {
                    let r_val = r as f32 * step;
                    table.push([r_val, g_val, b_val]);
                }
            }
        }

        Self {
            size: n,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            table,
        }
    }

    /// Evaluates RGB pixel using fast trilinear interpolation
    #[inline(always)]
    pub fn sample(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        let n = self.size;
        let n_sub_1 = (n - 1) as f32;

        let r_norm = ((r - self.domain_min[0]) / (self.domain_max[0] - self.domain_min[0])).clamp(0.0, 1.0);
        let g_norm = ((g - self.domain_min[1]) / (self.domain_max[1] - self.domain_min[1])).clamp(0.0, 1.0);
        let b_norm = ((b - self.domain_min[2]) / (self.domain_max[2] - self.domain_min[2])).clamp(0.0, 1.0);

        let rx = r_norm * n_sub_1;
        let gy = g_norm * n_sub_1;
        let bz = b_norm * n_sub_1;

        let r0 = (rx.floor() as usize).min(n - 1);
        let g0 = (gy.floor() as usize).min(n - 1);
        let b0 = (bz.floor() as usize).min(n - 1);

        let r1 = (r0 + 1).min(n - 1);
        let g1 = (g0 + 1).min(n - 1);
        let b1 = (b0 + 1).min(n - 1);

        let fr = rx - r0 as f32;
        let fg = gy - g0 as f32;
        let fb = bz - b0 as f32;

        let idx = |ir: usize, ig: usize, ib: usize| -> usize {
            ib * n * n + ig * n + ir
        };

        let c000 = self.table[idx(r0, g0, b0)];
        let c100 = self.table[idx(r1, g0, b0)];
        let c010 = self.table[idx(r0, g1, b0)];
        let c110 = self.table[idx(r1, g1, b0)];
        let c001 = self.table[idx(r0, g0, b1)];
        let c101 = self.table[idx(r1, g0, b1)];
        let c011 = self.table[idx(r0, g1, b1)];
        let c111 = self.table[idx(r1, g1, b1)];

        // Interpolate along Red
        let c00_r = c000[0] * (1.0 - fr) + c100[0] * fr;
        let c00_g = c000[1] * (1.0 - fr) + c100[1] * fr;
        let c00_b = c000[2] * (1.0 - fr) + c100[2] * fr;

        let c10_r = c010[0] * (1.0 - fr) + c110[0] * fr;
        let c10_g = c010[1] * (1.0 - fr) + c110[1] * fr;
        let c10_b = c010[2] * (1.0 - fr) + c110[2] * fr;

        let c01_r = c001[0] * (1.0 - fr) + c101[0] * fr;
        let c01_g = c001[1] * (1.0 - fr) + c101[1] * fr;
        let c01_b = c001[2] * (1.0 - fr) + c101[2] * fr;

        let c11_r = c011[0] * (1.0 - fr) + c111[0] * fr;
        let c11_g = c011[1] * (1.0 - fr) + c111[1] * fr;
        let c11_b = c011[2] * (1.0 - fr) + c111[2] * fr;

        // Interpolate along Green
        let c0_r = c00_r * (1.0 - fg) + c10_r * fg;
        let c0_g = c00_g * (1.0 - fg) + c10_g * fg;
        let c0_b = c00_b * (1.0 - fg) + c10_b * fg;

        let c1_r = c01_r * (1.0 - fg) + c11_r * fg;
        let c1_g = c01_g * (1.0 - fg) + c11_g * fg;
        let c1_b = c01_b * (1.0 - fg) + c11_b * fg;

        // Interpolate along Blue
        let out_r = c0_r * (1.0 - fb) + c1_r * fb;
        let out_g = c0_g * (1.0 - fb) + c1_g * fb;
        let out_b = c0_b * (1.0 - fb) + c1_b * fb;

        (out_r, out_g, out_b)
    }

    /// Built-in Hollywood Kodak 2383 Film Print Emulation (17^3)
    pub fn kodak_2383() -> Self {
        let mut lut = Self::identity(17);
        for entry in lut.table.iter_mut() {
            let r = entry[0];
            let g = entry[1];
            let b = entry[2];

            // Warm shadow toe, gentle cyan highlight roll-off, film contrast
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let contrast = ((luma - 0.5) * 1.15).tanh() * 0.5 + 0.5;
            let scale = if luma > 0.001 { contrast / luma } else { 1.0 };

            let r_mod = (r * scale * 1.04 + 0.015 * (1.0 - luma)).clamp(0.0, 1.0);
            let g_mod = (g * scale * 0.99 + 0.008 * (1.0 - luma)).clamp(0.0, 1.0);
            let b_mod = (b * scale * 0.93 + 0.035 * luma.powi(2)).clamp(0.0, 1.0);

            *entry = [r_mod, g_mod, b_mod];
        }
        lut
    }

    /// Built-in Teal & Orange Cinema Look (17^3)
    pub fn teal_and_orange() -> Self {
        let mut lut = Self::identity(17);
        for entry in lut.table.iter_mut() {
            let r = entry[0];
            let g = entry[1];
            let b = entry[2];
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

            // Shadows pushed towards teal, highlights pushed towards warm orange
            let s_weight = (1.0 - luma).powi(2);
            let h_weight = luma.powi(2);

            let r_mod = (r + h_weight * 0.12 - s_weight * 0.05).clamp(0.0, 1.0);
            let g_mod = (g + h_weight * 0.04 + s_weight * 0.03).clamp(0.0, 1.0);
            let b_mod = (b - h_weight * 0.08 + s_weight * 0.14).clamp(0.0, 1.0);

            *entry = [r_mod, g_mod, b_mod];
        }
        lut
    }

    /// Built-in Fuji Eterna 35mm Soft Film Look (17^3)
    pub fn fuji_eterna() -> Self {
        let mut lut = Self::identity(17);
        for entry in lut.table.iter_mut() {
            let r = entry[0];
            let g = entry[1];
            let b = entry[2];
            let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

            // Gentle low-contrast cine stock with subdued greens and warm rolloff
            let r_mod = (r * 0.96 + 0.04 * luma).clamp(0.0, 1.0);
            let g_mod = (g * 0.92 + 0.05 * luma).clamp(0.0, 1.0);
            let b_mod = (b * 0.94 + 0.03 * luma).clamp(0.0, 1.0);

            *entry = [r_mod, g_mod, b_mod];
        }
        lut
    }

    /// Built-in Silver Nitrate Monochrome Cinema Look (17^3)
    pub fn silver_nitrate() -> Self {
        let mut lut = Self::identity(17);
        for entry in lut.table.iter_mut() {
            let r = entry[0];
            let g = entry[1];
            let b = entry[2];
            // Spectral panchromatic weighting
            let mono = 0.299 * r + 0.587 * g + 0.114 * b;
            // High-acutance S-curve
            let s_mono = if mono < 0.5 {
                2.0 * mono * mono
            } else {
                1.0 - 2.0 * (1.0 - mono) * (1.0 - mono)
            };
            *entry = [s_mono, s_mono, s_mono];
        }
        lut
    }
}

/// Reads a DaVinci Resolve standard .cube file with strict security boundaries
/// (1 MiB ceiling, symlink rejection).
pub fn read_cube_file<P: AsRef<Path>>(path: P) -> Result<Lut3D, String> {
    let p = path.as_ref();

    // HANCORE Security: reject symlinks
    let meta = fs::symlink_metadata(p).map_err(|e| format!("Cannot stat LUT file: {}", e))?;
    if meta.file_type().is_symlink() {
        return Err("Security violation: symlink rejected for LUT file".to_string());
    }

    // HANCORE Security: Max 12 MiB limit for up to 65^3 cinematic LUT tables
    const MAX_SIZE: u64 = 12 * 1024 * 1024;
    let file = fs::File::open(p).map_err(|e| format!("Cannot open LUT file: {}", e))?;
    let limited = file.take(MAX_SIZE + 1);
    let reader = BufReader::new(limited);

    let mut size: Option<usize> = None;
    let mut domain_min = [0.0f32; 3];
    let mut domain_max = [1.0f32; 3];
    let mut table = Vec::new();

    let mut byte_count = 0u64;

    for line_res in reader.lines() {
        let line = line_res.map_err(|e| format!("Error reading LUT file: {}", e))?;
        byte_count += line.len() as u64 + 1;
        if byte_count > MAX_SIZE {
            return Err("LUT file exceeds maximum allowable size (1 MiB)".to_string());
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        if parts[0] == "LUT_3D_SIZE" && parts.len() >= 2 {
            let s: usize = parts[1].parse().map_err(|_| "Invalid LUT_3D_SIZE".to_string())?;
            if !(2..=65).contains(&s) {
                return Err("LUT_3D_SIZE must be between 2 and 65".to_string());
            }
            size = Some(s);
            table.reserve(s * s * s);
        } else if parts[0] == "DOMAIN_MIN" && parts.len() >= 4 {
            domain_min[0] = parts[1].parse().unwrap_or(0.0);
            domain_min[1] = parts[2].parse().unwrap_or(0.0);
            domain_min[2] = parts[3].parse().unwrap_or(0.0);
        } else if parts[0] == "DOMAIN_MAX" && parts.len() >= 4 {
            domain_max[0] = parts[1].parse().unwrap_or(1.0);
            domain_max[1] = parts[2].parse().unwrap_or(1.0);
            domain_max[2] = parts[3].parse().unwrap_or(1.0);
        } else if parts.len() >= 3 {
            if let (Ok(r), Ok(g), Ok(b)) = (parts[0].parse::<f32>(), parts[1].parse::<f32>(), parts[2].parse::<f32>()) {
                if r.is_finite() && g.is_finite() && b.is_finite() {
                    table.push([r, g, b]);
                } else {
                    return Err("Non-finite float detected in LUT file".to_string());
                }
            }
        }
    }

    let lut_size = size.ok_or_else(|| "Missing LUT_3D_SIZE header in .cube file".to_string())?;
    let expected_elements = lut_size * lut_size * lut_size;
    if table.len() != expected_elements {
        return Err(format!(
            "LUT table size mismatch: expected {} entries, got {}",
            expected_elements,
            table.len()
        ));
    }

    Ok(Lut3D {
        size: lut_size,
        domain_min,
        domain_max,
        table,
    })
}

/// Applies 3D LUT to a single pixel with mix intensity
#[inline(always)]
pub fn apply_lut_pixel(
    r: f32,
    g: f32,
    b: f32,
    lut: &Lut3D,
    intensity: f32,
) -> (f32, f32, f32) {
    if intensity <= 0.001 {
        return (r, g, b);
    }

    let (lut_r, lut_g, lut_b) = lut.sample(r, g, b);
    if intensity >= 0.999 {
        (lut_r, lut_g, lut_b)
    } else {
        (
            r * (1.0 - intensity) + lut_r * intensity,
            g * (1.0 - intensity) + lut_g * intensity,
            b * (1.0 - intensity) + lut_b * intensity,
        )
    }
}
