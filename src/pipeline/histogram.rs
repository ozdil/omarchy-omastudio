use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramData {
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub luma: Vec<u32>,
    pub max_count: u32,
    pub shadow_clipping_percent: f32,
    pub highlight_clipping_percent: f32,

    // DaVinci Resolve Video Scopes (Normalized 0..255 glow intensity)
    #[serde(default)]
    pub waveform_luma: Vec<u8>, // 64 cols x 32 rows flat array
    #[serde(default)]
    pub parade_r: Vec<u8>,      // 32 cols x 32 rows
    #[serde(default)]
    pub parade_g: Vec<u8>,      // 32 cols x 32 rows
    #[serde(default)]
    pub parade_b: Vec<u8>,      // 32 cols x 32 rows
    #[serde(default)]
    pub vectorscope: Vec<u8>,   // 48 x 48 UV / CbCr chromaticity grid
}

impl Default for HistogramData {
    fn default() -> Self {
        Self {
            red: vec![0; 256],
            green: vec![0; 256],
            blue: vec![0; 256],
            luma: vec![0; 256],
            max_count: 1,
            shadow_clipping_percent: 0.0,
            highlight_clipping_percent: 0.0,
            waveform_luma: vec![0; 64 * 32],
            parade_r: vec![0; 32 * 32],
            parade_g: vec![0; 32 * 32],
            parade_b: vec![0; 32 * 32],
            vectorscope: vec![0; 48 * 48],
        }
    }
}

pub fn compute_histogram(rgb_buffer: &[u8], channels: usize) -> HistogramData {
    let total_pixels = rgb_buffer.len() / channels;
    let w = (total_pixels as f32).sqrt().round() as usize;
    let h = total_pixels.checked_div(w).unwrap_or(1);
    compute_histogram_with_dimensions(rgb_buffer, w, h, channels)
}

pub fn compute_histogram_with_dimensions(
    rgb_buffer: &[u8],
    width: usize,
    _height: usize,
    channels: usize,
) -> HistogramData {
    let mut red = vec![0u32; 256];
    let mut green = vec![0u32; 256];
    let mut blue = vec![0u32; 256];
    let mut luma = vec![0u32; 256];

    let mut shadow_clips = 0u32;
    let mut highlight_clips = 0u32;
    let total_pixels = rgb_buffer.len() / channels;
    let w_safe = width.max(1);

    // Scopes accumulation buffers
    let mut wf_counts = vec![0u32; 64 * 32];
    let mut par_r_counts = vec![0u32; 32 * 32];
    let mut par_g_counts = vec![0u32; 32 * 32];
    let mut par_b_counts = vec![0u32; 32 * 32];
    let mut vec_counts = vec![0u32; 48 * 48];

    for (px_idx, chunk) in rgb_buffer.chunks_exact(channels).enumerate() {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];
        let y = ((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) as u32).min(255) as u8;

        red[r as usize] += 1;
        green[g as usize] += 1;
        blue[b as usize] += 1;
        luma[y as usize] += 1;

        if y <= 1 {
            shadow_clips += 1;
        } else if y >= 254 {
            highlight_clips += 1;
        }

        let x = px_idx % w_safe;

        // Waveform Luma (64 cols x 32 rows, 0 IRE at bottom, 100 IRE at top)
        let wx = (x * 64) / w_safe;
        let wy = 31usize.saturating_sub((y as usize * 31) / 255);
        wf_counts[wy * 64 + wx.min(63)] += 1;

        // RGB Parade (32 cols x 32 rows, 0 at bottom, 255 at top)
        let px = (x * 32) / w_safe;
        let ry = 31usize.saturating_sub((r as usize * 31) / 255);
        let gy = 31usize.saturating_sub((g as usize * 31) / 255);
        let by = 31usize.saturating_sub((b as usize * 31) / 255);
        par_r_counts[ry * 32 + px.min(31)] += 1;
        par_g_counts[gy * 32 + px.min(31)] += 1;
        par_b_counts[by * 32 + px.min(31)] += 1;

        // Vectorscope (48 x 48 UV / CbCr grid, +Cr Red at top)
        let rf = r as f32 / 255.0;
        let gf = g as f32 / 255.0;
        let bf = b as f32 / 255.0;
        let cb = -0.1146 * rf - 0.3854 * gf + 0.5000 * bf;
        let cr = 0.5000 * rf - 0.4542 * gf - 0.0458 * bf;
        let u = ((cb + 0.5) * 47.0).clamp(0.0, 47.0) as usize;
        let v = (47.0 - (cr + 0.5) * 47.0).clamp(0.0, 47.0) as usize;
        vec_counts[v * 48 + u] += 1;
    }

    // Determine max peak excluding absolute 0 and 255 spikes
    let mut max_count = 1;
    for i in 1..255 {
        max_count = max_count
            .max(red[i])
            .max(green[i])
            .max(blue[i])
            .max(luma[i]);
    }

    let shadow_percent = if total_pixels > 0 {
        (shadow_clips as f32 / total_pixels as f32) * 100.0
    } else {
        0.0
    };

    let highlight_percent = if total_pixels > 0 {
        (highlight_clips as f32 / total_pixels as f32) * 100.0
    } else {
        0.0
    };

    // Normalize scope counts using soft square-root curve (analog phosphor response)
    let normalize_counts = |counts: &[u32]| -> Vec<u8> {
        let max_val = *counts.iter().max().unwrap_or(&1).max(&1) as f32;
        counts
            .iter()
            .map(|&c| {
                if c == 0 {
                    0u8
                } else {
                    ((c as f32 / max_val).sqrt() * 255.0).clamp(1.0, 255.0) as u8
                }
            })
            .collect()
    };

    let waveform_luma = normalize_counts(&wf_counts);
    let parade_r = normalize_counts(&par_r_counts);
    let parade_g = normalize_counts(&par_g_counts);
    let parade_b = normalize_counts(&par_b_counts);
    let vectorscope = normalize_counts(&vec_counts);

    HistogramData {
        red,
        green,
        blue,
        luma,
        max_count,
        shadow_clipping_percent: shadow_percent,
        highlight_clipping_percent: highlight_percent,
        waveform_luma,
        parade_r,
        parade_g,
        parade_b,
        vectorscope,
    }
}

pub fn compute_histogram_16(rgb_buffer: &[u16], channels: usize) -> HistogramData {
    let total_pixels = rgb_buffer.len() / channels;
    let w = (total_pixels as f32).sqrt().round() as usize;
    let h = total_pixels.checked_div(w).unwrap_or(1);
    compute_histogram_16_with_dimensions(rgb_buffer, w, h, channels)
}

pub fn compute_histogram_16_with_dimensions(
    rgb_buffer: &[u16],
    width: usize,
    _height: usize,
    channels: usize,
) -> HistogramData {
    let mut red = vec![0u32; 256];
    let mut green = vec![0u32; 256];
    let mut blue = vec![0u32; 256];
    let mut luma = vec![0u32; 256];

    let mut shadow_clips = 0u32;
    let mut highlight_clips = 0u32;
    let total_pixels = rgb_buffer.len() / channels;
    let w_safe = width.max(1);

    let mut wf_counts = vec![0u32; 64 * 32];
    let mut par_r_counts = vec![0u32; 32 * 32];
    let mut par_g_counts = vec![0u32; 32 * 32];
    let mut par_b_counts = vec![0u32; 32 * 32];
    let mut vec_counts = vec![0u32; 48 * 48];

    for (px_idx, chunk) in rgb_buffer.chunks_exact(channels).enumerate() {
        let r = (chunk[0] >> 8) as u8;
        let g = (chunk[1] >> 8) as u8;
        let b = (chunk[2] >> 8) as u8;
        let y = ((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) as u32).min(255) as u8;

        red[r as usize] += 1;
        green[g as usize] += 1;
        blue[b as usize] += 1;
        luma[y as usize] += 1;

        if y <= 1 {
            shadow_clips += 1;
        } else if y >= 254 {
            highlight_clips += 1;
        }

        let x = px_idx % w_safe;

        let wx = (x * 64) / w_safe;
        let wy = 31usize.saturating_sub((y as usize * 31) / 255);
        wf_counts[wy * 64 + wx.min(63)] += 1;

        let px = (x * 32) / w_safe;
        let ry = 31usize.saturating_sub((r as usize * 31) / 255);
        let gy = 31usize.saturating_sub((g as usize * 31) / 255);
        let by = 31usize.saturating_sub((b as usize * 31) / 255);
        par_r_counts[ry * 32 + px.min(31)] += 1;
        par_g_counts[gy * 32 + px.min(31)] += 1;
        par_b_counts[by * 32 + px.min(31)] += 1;

        let rf = r as f32 / 255.0;
        let gf = g as f32 / 255.0;
        let bf = b as f32 / 255.0;
        let cb = -0.1146 * rf - 0.3854 * gf + 0.5000 * bf;
        let cr = 0.5000 * rf - 0.4542 * gf - 0.0458 * bf;
        let u = ((cb + 0.5) * 47.0).clamp(0.0, 47.0) as usize;
        let v = (47.0 - (cr + 0.5) * 47.0).clamp(0.0, 47.0) as usize;
        vec_counts[v * 48 + u] += 1;
    }

    let mut max_count = 1;
    for i in 1..255 {
        max_count = max_count
            .max(red[i])
            .max(green[i])
            .max(blue[i])
            .max(luma[i]);
    }

    let shadow_percent = if total_pixels > 0 {
        (shadow_clips as f32 / total_pixels as f32) * 100.0
    } else {
        0.0
    };

    let highlight_percent = if total_pixels > 0 {
        (highlight_clips as f32 / total_pixels as f32) * 100.0
    } else {
        0.0
    };

    let normalize_counts = |counts: &[u32]| -> Vec<u8> {
        let max_val = *counts.iter().max().unwrap_or(&1).max(&1) as f32;
        counts
            .iter()
            .map(|&c| {
                if c == 0 {
                    0u8
                } else {
                    ((c as f32 / max_val).sqrt() * 255.0).clamp(1.0, 255.0) as u8
                }
            })
            .collect()
    };

    let waveform_luma = normalize_counts(&wf_counts);
    let parade_r = normalize_counts(&par_r_counts);
    let parade_g = normalize_counts(&par_g_counts);
    let parade_b = normalize_counts(&par_b_counts);
    let vectorscope = normalize_counts(&vec_counts);

    HistogramData {
        red,
        green,
        blue,
        luma,
        max_count,
        shadow_clipping_percent: shadow_percent,
        highlight_clipping_percent: highlight_percent,
        waveform_luma,
        parade_r,
        parade_g,
        parade_b,
        vectorscope,
    }
}
