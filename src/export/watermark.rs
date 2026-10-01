//! Professional Photography Watermark Rendering Engine
//!
//! Provides selectable, high-fidelity text and logo watermarking:
//! - 9-Point grid anchor alignment (Top-Left to Bottom-Right)
//! - Crisp scalable typography with drop-shadow contrast enhancement
//! - External logo overlay blending with Lanczos3 resampling
//! - EXIF metadata tag interpolation ({camera}, {lens}, {iso}, {aperture}, {shutter})
//! - 16-bit and 8-bit image buffer support

use crate::raw::RawMetadata;
use image::DynamicImage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WatermarkOptions {
    pub enabled: bool,
    pub watermark_type: String, // "text" or "logo"
    pub text: String,
    pub logo_path: Option<String>,
    pub position_index: u32, // 0..8 (9-point grid: 0=top-left, 4=center, 8=bottom-right)
    pub opacity: f32, // 0.1 to 1.0
    pub size: u32, // font size multiplier (8..36)
    pub margin: u32, // margin in pixels (8..80)
    pub color: String, // hex e.g. "#ffffff"
    pub drop_shadow: bool,
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            watermark_type: "text".to_string(),
            text: "OmaStudio Photography".to_string(),
            logo_path: None,
            position_index: 8, // Bottom-right default
            opacity: 0.85,
            size: 14,
            margin: 24,
            color: "#ffffff".to_string(),
            drop_shadow: true,
        }
    }
}

/// Resolves effective watermark text by formatting EXIF tags if present
pub fn resolve_watermark_text(template: &str, meta: Option<&RawMetadata>) -> String {
    let mut text = template.to_string();
    if let Some(m) = meta {
        let cam = if !m.make.is_empty() && !m.model.is_empty() {
            format!("{} {}", m.make, m.model)
        } else if !m.model.is_empty() {
            m.model.clone()
        } else {
            "OmaStudio".to_string()
        };

        text = text.replace("{camera}", &cam);
        text = text.replace("{lens}", &m.lens);
        text = text.replace("{iso}", &format!("ISO {}", m.iso as u32));
        text = text.replace("{aperture}", &format!("f/{:.1}", m.aperture));

        let shutter_str = if m.shutter < 1.0 && m.shutter > 0.0 {
            format!("1/{}s", (1.0 / m.shutter).round() as u32)
        } else {
            format!("{:.1}s", m.shutter)
        };
        text = text.replace("{shutter}", &shutter_str);
        text = text.replace("{focal}", &format!("{}mm", m.focal_length.round() as u32));
    }
    text
}

/// Parses hex color string (e.g. "#ffffff", "#c0caf5") to RGB u8
fn parse_hex_color(hex: &str) -> (u8, u8, u8) {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&clean[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&clean[4..6], 16).unwrap_or(255);
        (r, g, b)
    } else {
        (255, 255, 255)
    }
}

/// Minimalist, crystal-clear 5x7 procedural bitmap glyph matrix for ASCII 32..126
fn get_glyph_bitmap(c: char) -> [u8; 7] {
    match c {
        'A' | 'a' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' | 'b' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' | 'c' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
        'D' | 'd' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' | 'e' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' | 'f' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' | 'g' => [0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' | 'h' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' | 'i' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' | 'j' => [0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110],
        'K' | 'k' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' | 'l' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' | 'm' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' | 'n' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' | 'o' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' | 'p' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' | 'q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10011, 0b01111],
        'R' | 'r' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' | 's' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' | 't' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' | 'u' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' | 'v' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' | 'w' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' | 'x' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' | 'y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' | 'z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        ':' => [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '|' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        '[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        ']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        _ => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000], // Space or unsupported
    }
}

/// Renders a text string into an RGBA pixel buffer
fn render_text_to_rgba(
    text: &str,
    scale: u32,
    fg: (u8, u8, u8),
    drop_shadow: bool,
) -> (Vec<u8>, u32, u32) {
    let char_w = 6 * scale; // 5 px glyph + 1 px spacing
    let char_h = 8 * scale; // 7 px glyph + 1 px spacing

    let num_chars = text.chars().count() as u32;
    let shadow_offset = if drop_shadow { (scale / 2).max(1) } else { 0 };

    let total_w = num_chars * char_w + shadow_offset * 2;
    let total_h = char_h + shadow_offset * 2;

    let mut buf = vec![0u8; (total_w * total_h * 4) as usize];

    let plot_px = |buf: &mut Vec<u8>, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8| {
        if x < total_w && y < total_h {
            let idx = ((y * total_w + x) * 4) as usize;
            // Standard alpha over
            let cur_a = buf[idx + 3] as f32 / 255.0;
            let new_a = a as f32 / 255.0;
            let out_a = new_a + cur_a * (1.0 - new_a);

            if out_a > 0.0 {
                let out_r = (r as f32 * new_a + buf[idx] as f32 * cur_a * (1.0 - new_a)) / out_a;
                let out_g = (g as f32 * new_a + buf[idx + 1] as f32 * cur_a * (1.0 - new_a)) / out_a;
                let out_b = (b as f32 * new_a + buf[idx + 2] as f32 * cur_a * (1.0 - new_a)) / out_a;

                buf[idx] = out_r as u8;
                buf[idx + 1] = out_g as u8;
                buf[idx + 2] = out_b as u8;
                buf[idx + 3] = (out_a * 255.0) as u8;
            }
        }
    };

    // Draw Drop Shadow First
    if drop_shadow {
        for (i, c) in text.chars().enumerate() {
            let matrix = get_glyph_bitmap(c);
            let start_x = (i as u32) * char_w + shadow_offset;
            let start_y = shadow_offset;

            for (row_idx, &row_bits) in matrix.iter().enumerate() {
                for col_idx in 0..5 {
                    if (row_bits & (1 << (4 - col_idx))) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                plot_px(
                                    &mut buf,
                                    start_x + (col_idx as u32) * scale + sx,
                                    start_y + (row_idx as u32) * scale + sy,
                                    0, 0, 0, 200,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Draw Main Text
    for (i, c) in text.chars().enumerate() {
        let matrix = get_glyph_bitmap(c);
        let start_x = (i as u32) * char_w;
        let start_y = 0;

        for (row_idx, &row_bits) in matrix.iter().enumerate() {
            for col_idx in 0..5 {
                if (row_bits & (1 << (4 - col_idx))) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            plot_px(
                                &mut buf,
                                start_x + (col_idx as u32) * scale + sx,
                                start_y + (row_idx as u32) * scale + sy,
                                fg.0, fg.1, fg.2, 255,
                            );
                        }
                    }
                }
            }
        }
    }

    (buf, total_w, total_h)
}

/// Applies watermark (text or logo) directly onto an image buffer
pub fn apply_watermark(
    img: &mut DynamicImage,
    opts: &WatermarkOptions,
    meta: Option<&RawMetadata>,
) {
    if !opts.enabled {
        return;
    }

    let (img_w, img_h) = (img.width(), img.height());
    if img_w < 50 || img_h < 50 {
        return;
    }

    // Determine watermark graphic (RGBA buffer)
    let (wm_buf, wm_w, wm_h) = if opts.watermark_type == "logo" {
        if let Some(ref path) = opts.logo_path {
            if let Ok(logo_img) = image::open(path) {
                let target_w = (img_w as f32 * 0.15 * (opts.size as f32 / 14.0)).round() as u32;
                let resized = logo_img.resize(target_w.max(32), target_w.max(32), image::imageops::FilterType::Lanczos3);
                let rgba = resized.to_rgba8();
                let w = rgba.width();
                let h = rgba.height();
                (rgba.into_raw(), w, h)
            } else {
                let text = resolve_watermark_text(&opts.text, meta);
                let fg = parse_hex_color(&opts.color);
                let scale = ((img_w.max(img_h) as f32 / 1200.0) * (opts.size as f32 / 14.0)).round().max(1.0) as u32;
                render_text_to_rgba(&text, scale, fg, opts.drop_shadow)
            }
        } else {
            let text = resolve_watermark_text(&opts.text, meta);
            let fg = parse_hex_color(&opts.color);
            let scale = ((img_w.max(img_h) as f32 / 1200.0) * (opts.size as f32 / 14.0)).round().max(1.0) as u32;
            render_text_to_rgba(&text, scale, fg, opts.drop_shadow)
        }
    } else {
        let text = resolve_watermark_text(&opts.text, meta);
        let fg = parse_hex_color(&opts.color);
        let scale = ((img_w.max(img_h) as f32 / 1200.0) * (opts.size as f32 / 14.0)).round().max(1.0) as u32;
        render_text_to_rgba(&text, scale, fg, opts.drop_shadow)
    };

    if wm_w == 0 || wm_h == 0 {
        return;
    }

    // 9-Point Grid Positioning
    let margin = opts.margin.max(4);
    let target_x = match opts.position_index {
        0 | 3 | 6 => margin,                                       // Left
        1 | 4 | 7 => img_w.saturating_sub(wm_w) / 2,               // Center
        _ => img_w.saturating_sub(wm_w + margin),                  // Right (default: 8)
    };

    let target_y = match opts.position_index {
        0..=2 => margin,                                       // Top
        3..=5 => img_h.saturating_sub(wm_h) / 2,               // Middle
        _ => img_h.saturating_sub(wm_h + margin),              // Bottom (default: 8)
    };

    let opacity = opts.opacity.clamp(0.05, 1.0);

    // Alpha blend watermark onto image
    match img {
        DynamicImage::ImageRgb8(ref mut rgb8) => {
            for y in 0..wm_h {
                let dst_y = target_y + y;
                if dst_y >= img_h {
                    continue;
                }
                for x in 0..wm_w {
                    let dst_x = target_x + x;
                    if dst_x >= img_w {
                        continue;
                    }
                    let wm_idx = ((y * wm_w + x) * 4) as usize;
                    let wm_a = (wm_buf[wm_idx + 3] as f32 / 255.0) * opacity;

                    if wm_a > 0.001 {
                        let px = rgb8.get_pixel_mut(dst_x, dst_y);
                        px[0] = (px[0] as f32 * (1.0 - wm_a) + wm_buf[wm_idx] as f32 * wm_a).round() as u8;
                        px[1] = (px[1] as f32 * (1.0 - wm_a) + wm_buf[wm_idx + 1] as f32 * wm_a).round() as u8;
                        px[2] = (px[2] as f32 * (1.0 - wm_a) + wm_buf[wm_idx + 2] as f32 * wm_a).round() as u8;
                    }
                }
            }
        }
        DynamicImage::ImageRgb16(ref mut rgb16) => {
            for y in 0..wm_h {
                let dst_y = target_y + y;
                if dst_y >= img_h {
                    continue;
                }
                for x in 0..wm_w {
                    let dst_x = target_x + x;
                    if dst_x >= img_w {
                        continue;
                    }
                    let wm_idx = ((y * wm_w + x) * 4) as usize;
                    let wm_a = (wm_buf[wm_idx + 3] as f32 / 255.0) * opacity;

                    if wm_a > 0.001 {
                        let px = rgb16.get_pixel_mut(dst_x, dst_y);
                        let wm_r = (wm_buf[wm_idx] as u16) << 8;
                        let wm_g = (wm_buf[wm_idx + 1] as u16) << 8;
                        let wm_b = (wm_buf[wm_idx + 2] as u16) << 8;

                        px[0] = (px[0] as f32 * (1.0 - wm_a) + wm_r as f32 * wm_a).round() as u16;
                        px[1] = (px[1] as f32 * (1.0 - wm_a) + wm_g as f32 * wm_a).round() as u16;
                        px[2] = (px[2] as f32 * (1.0 - wm_a) + wm_b as f32 * wm_a).round() as u16;
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watermark_text_rendering() {
        let text = "OmaStudio 2026";
        let (buf, w, h) = render_text_to_rgba(text, 2, (255, 255, 255), true);
        assert!(w > 0);
        assert!(h > 0);
        assert_eq!(buf.len(), (w * h * 4) as usize);

        // Verify that there are non-zero alpha pixels
        let has_visible_pixels = buf.iter().skip(3).step_by(4).any(|&p| p > 0);
        assert!(has_visible_pixels, "Rendered watermark must contain visible pixels");
    }

    #[test]
    fn test_exif_tag_interpolation() {
        let meta = RawMetadata {
            width: 100,
            height: 100,
            raw_width: 100,
            raw_height: 100,
            make: "Fujifilm".to_string(),
            model: "GFX 100 II".to_string(),
            lens: "GF 110mm F2 R LM WR".to_string(),
            iso: 100.0,
            shutter: 0.002, // 1/500s
            aperture: 2.0,
            focal_length: 110.0,
            timestamp: 1700000000,
            cam_mul: [1.0, 1.0, 1.0, 1.0],
        };

        let template = "{camera} | {lens} | {aperture} | {shutter} | {iso}";
        let res = resolve_watermark_text(template, Some(&meta));
        assert!(res.contains("Fujifilm GFX 100 II"));
        assert!(res.contains("GF 110mm F2 R LM WR"));
        assert!(res.contains("f/2.0"));
        assert!(res.contains("1/500s"));
        assert!(res.contains("ISO 100"));
    }

    #[test]
    fn test_apply_watermark_to_rgb16() {
        let mut img = DynamicImage::new_rgb16(200, 200);
        let opts = WatermarkOptions {
            enabled: true,
            watermark_type: "text".to_string(),
            text: "TEST".to_string(),
            logo_path: None,
            position_index: 8,
            opacity: 0.9,
            size: 14,
            margin: 10,
            color: "#ffffff".to_string(),
            drop_shadow: true,
        };

        apply_watermark(&mut img, &opts, None);
        // Verify image contains non-black pixels in bottom-right corner
        let rgb16 = img.as_rgb16().unwrap();
        let mut non_zero_count = 0;
        for y in 100..200 {
            for x in 100..200 {
                let px = rgb16.get_pixel(x, y);
                if px[0] > 0 || px[1] > 0 || px[2] > 0 {
                    non_zero_count += 1;
                }
            }
        }
        assert!(non_zero_count > 0, "Watermark must draw pixels onto the image buffer");
    }
}
