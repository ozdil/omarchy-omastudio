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

        let shutter_str = if !m.shutter.is_finite() || m.shutter <= 0.0 {
            String::new()
        } else if m.shutter < 1.0 {
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
    if clean.is_ascii() && clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&clean[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&clean[4..6], 16).unwrap_or(255);
        (r, g, b)
    } else {
        (255, 255, 255)
    }
}

/// Procedural 5x9 bitmap glyph matrix (row 0 = top diacritic, rows 1..=7 = 5x7 body,
/// row 8 = bottom cedilla/descender) covering ASCII, photography symbols (©, @, _, ,, °, ·)
/// and Turkish / common Latin diacritics.
fn get_glyph_bitmap(c: char) -> [u8; 9] {
    let body = |b: [u8; 7]| [0, b[0], b[1], b[2], b[3], b[4], b[5], b[6], 0];
    let with_top = |top: u8, b: [u8; 7]| [top, b[0], b[1], b[2], b[3], b[4], b[5], b[6], 0];
    let with_cedilla = |b: [u8; 7]| [0, b[0], b[1], b[2], b[3], b[4], b[5], b[6], 0b00100];

    let g_a = [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001];
    let g_c = [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111];
    let g_e = [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111];
    let g_g = [0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111];
    let g_i = [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110];
    let g_n = [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001];
    let g_o = [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110];
    let g_s = [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110];
    let g_u = [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110];
    let g_y = [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100];

    match c {
        'A' | 'a' => body(g_a),
        'B' | 'b' => body([0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
        'C' | 'c' => body(g_c),
        'D' | 'd' => body([0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
        'E' | 'e' => body(g_e),
        'F' | 'f' => body([0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
        'G' | 'g' => body(g_g),
        'H' | 'h' => body([0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
        'I' | 'i' | 'ı' => body(g_i),
        'İ' => with_top(0b00100, g_i),
        'J' | 'j' => body([0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110]),
        'K' | 'k' => body([0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
        'L' | 'l' => body([0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
        'M' | 'm' => body([0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
        'N' | 'n' => body(g_n),
        'O' | 'o' => body(g_o),
        'P' | 'p' => body([0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
        'Q' | 'q' => body([0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10011, 0b01111]),
        'R' | 'r' => body([0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
        'S' | 's' => body(g_s),
        'T' | 't' => body([0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
        'U' | 'u' => body(g_u),
        'V' | 'v' => body([0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
        'W' | 'w' => body([0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001]),
        'X' | 'x' => body([0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
        'Y' | 'y' => body(g_y),
        'Z' | 'z' => body([0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
        // Turkish and common Latin diacritics
        'Ç' | 'ç' => with_cedilla(g_c),
        'Ğ' | 'ğ' => with_top(0b01110, g_g),
        'Ö' | 'ö' => with_top(0b01010, g_o),
        'Ş' | 'ş' => with_cedilla(g_s),
        'Ü' | 'ü' => with_top(0b01010, g_u),
        'Ä' | 'ä' => with_top(0b01010, g_a),
        'Å' | 'å' => with_top(0b00100, g_a),
        'À' | 'Á' | 'Â' | 'Ã' | 'à' | 'á' | 'â' | 'ã' => with_top(0b00110, g_a),
        'É' | 'È' | 'Ê' | 'Ë' | 'é' | 'è' | 'ê' | 'ë' => with_top(0b00110, g_e),
        'Í' | 'Ì' | 'Î' | 'Ï' | 'í' | 'ì' | 'î' | 'ï' => with_top(0b00110, g_i),
        'Ñ' | 'ñ' => with_top(0b01110, g_n),
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'ó' | 'ò' | 'ô' | 'õ' => with_top(0b00110, g_o),
        'Ø' | 'ø' => body([0b01111, 0b10011, 0b10101, 0b10101, 0b11001, 0b11001, 0b11110]),
        'Ú' | 'Ù' | 'Û' | 'ú' | 'ù' | 'û' => with_top(0b00110, g_u),
        'Ý' | 'ý' | 'ÿ' => with_top(0b01010, g_y),
        'ß' => body([0b01110, 0b10001, 0b10010, 0b10100, 0b10010, 0b10001, 0b10110]),
        // Digits
        '0' => body([0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110]),
        '1' => body([0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
        '2' => body([0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
        '3' => body([0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110]),
        '4' => body([0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
        '5' => body([0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
        '6' => body([0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110]),
        '7' => body([0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
        '8' => body([0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
        '9' => body([0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110]),
        // Punctuation & symbols
        '.' => body([0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100]),
        ',' => [0, 0, 0, 0, 0, 0, 0b00110, 0b00110, 0b01100],
        ':' => body([0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000]),
        ';' => [0, 0, 0b00110, 0b00110, 0, 0, 0b00110, 0b00110, 0b01100],
        '/' => body([0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000]),
        '\\' => body([0b10000, 0b01000, 0b00100, 0b00010, 0b00001, 0b00000, 0b00000]),
        '-' | '–' | '—' => body([0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000]),
        '_' => [0, 0, 0, 0, 0, 0, 0, 0b11111, 0],
        '|' => body([0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
        '[' => body([0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110]),
        ']' => body([0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110]),
        '(' => body([0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010]),
        ')' => body([0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000]),
        '@' => body([0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01111]),
        '©' => body([0b01110, 0b10001, 0b10110, 0b10100, 0b10110, 0b10001, 0b01110]),
        '®' => body([0b01110, 0b10001, 0b11101, 0b11011, 0b11101, 0b10001, 0b01110]),
        '!' => body([0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100]),
        '?' => body([0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100]),
        '&' => body([0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101]),
        '+' => body([0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000]),
        '=' => body([0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000]),
        '#' => body([0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010]),
        '%' => body([0b11001, 0b11010, 0b00100, 0b01000, 0b10110, 0b00110, 0b00000]),
        '*' => body([0b00000, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0b00000]),
        '\'' => body([0b00100, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000]),
        '"' => body([0b01010, 0b01010, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000]),
        '°' => body([0b00110, 0b01001, 0b00110, 0b00000, 0b00000, 0b00000, 0b00000]),
        '·' | '•' => body([0b00000, 0b00000, 0b00100, 0b01110, 0b00100, 0b00000, 0b00000]),
        '~' => body([0b00000, 0b00000, 0b01001, 0b10110, 0b00000, 0b00000, 0b00000]),
        _ => [0; 9], // Space or unsupported
    }
}

const MAX_WATERMARK_CHARS: usize = 256;
const MAX_WATERMARK_BYTES: usize = 64 * 1024 * 1024;

/// Renders a text string into an RGBA pixel buffer with overflow and memory bounds.
fn render_text_to_rgba(
    text: &str,
    scale: u32,
    fg: (u8, u8, u8),
    drop_shadow: bool,
) -> (Vec<u8>, u32, u32) {
    let scale = scale.clamp(1, 64);
    let chars: Vec<char> = text.chars().take(MAX_WATERMARK_CHARS).collect();
    if chars.is_empty() {
        return (Vec::new(), 0, 0);
    }

    let char_w = 6u32.saturating_mul(scale); // 5 px glyph + 1 px spacing
    let char_h = 10u32.saturating_mul(scale); // 9 px glyph + 1 px spacing
    let shadow_offset = if drop_shadow { (scale / 2).max(1) } else { 0 };

    let Some(total_w) = (chars.len() as u32)
        .checked_mul(char_w)
        .and_then(|w| w.checked_add(shadow_offset.saturating_mul(2)))
    else {
        return (Vec::new(), 0, 0);
    };
    let Some(total_h) = char_h.checked_add(shadow_offset.saturating_mul(2)) else {
        return (Vec::new(), 0, 0);
    };
    let Some(buf_len) = (total_w as usize)
        .checked_mul(total_h as usize)
        .and_then(|px| px.checked_mul(4))
    else {
        return (Vec::new(), 0, 0);
    };
    if buf_len == 0 || buf_len > MAX_WATERMARK_BYTES {
        return (Vec::new(), 0, 0);
    }

    let mut buf = vec![0u8; buf_len];

    let plot_px = |buf: &mut Vec<u8>, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8| {
        if x < total_w && y < total_h {
            let idx = ((y as usize) * (total_w as usize) + (x as usize)) * 4;
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
        for (i, &c) in chars.iter().enumerate() {
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
    for (i, &c) in chars.iter().enumerate() {
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

    let clamped_size = opts.size.clamp(4, 128);
    let render_text_fallback = || {
        let text = resolve_watermark_text(&opts.text, meta);
        let fg = parse_hex_color(&opts.color);
        let scale = ((img_w.max(img_h) as f32 / 1200.0) * (clamped_size as f32 / 14.0))
            .round()
            .clamp(1.0, 64.0) as u32;
        render_text_to_rgba(&text, scale, fg, opts.drop_shadow)
    };

    // Determine watermark graphic (RGBA buffer)
    let (wm_buf, wm_w, wm_h) = if opts.watermark_type == "logo" {
        if let Some(ref path) = opts.logo_path {
            let p = std::path::Path::new(path);
            // Atomic O_NOFOLLOW + size-bounded read eliminates symlink TOCTOU
            let logo = crate::security::read_secure_file(p, 50 * 1024 * 1024)
                .ok()
                .and_then(|bytes| image::load_from_memory(&bytes).ok());
            if let Some(logo_img) = logo {
                let target_w = ((img_w as f32 * 0.15 * (clamped_size as f32 / 14.0)).round() as u32)
                    .clamp(32, img_w.max(32));
                let resized = logo_img.resize(target_w, target_w, image::imageops::FilterType::Lanczos3);
                let rgba = resized.to_rgba8();
                let w = rgba.width();
                let h = rgba.height();
                (rgba.into_raw(), w, h)
            } else {
                render_text_fallback()
            }
        } else {
            render_text_fallback()
        }
    } else {
        render_text_fallback()
    };

    if wm_w == 0 || wm_h == 0 {
        return;
    }

    // 9-Point Grid Positioning
    let margin = opts.margin.max(4);
    let target_x = match opts.position_index {
        0 | 3 | 6 => margin,                                       // Left
        1 | 4 | 7 => img_w.saturating_sub(wm_w) / 2,               // Center
        _ => img_w.saturating_sub(wm_w.saturating_add(margin)),    // Right (default: 8)
    };

    let target_y = match opts.position_index {
        0..=2 => margin,                                           // Top
        3..=5 => img_h.saturating_sub(wm_h) / 2,                   // Middle
        _ => img_h.saturating_sub(wm_h.saturating_add(margin)),    // Bottom (default: 8)
    };

    let opacity = if opts.opacity.is_finite() { opts.opacity.clamp(0.05, 1.0) } else { 0.85 };

    // Alpha blend watermark onto image
    match img {
        DynamicImage::ImageRgb8(rgb8) => {
            for y in 0..wm_h {
                let dst_y = target_y.saturating_add(y);
                if dst_y >= img_h {
                    continue;
                }
                for x in 0..wm_w {
                    let dst_x = target_x.saturating_add(x);
                    if dst_x >= img_w {
                        continue;
                    }
                    let wm_idx = ((y as usize) * (wm_w as usize) + (x as usize)) * 4;
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
        DynamicImage::ImageRgb16(rgb16) => {
            for y in 0..wm_h {
                let dst_y = target_y.saturating_add(y);
                if dst_y >= img_h {
                    continue;
                }
                for x in 0..wm_w {
                    let dst_x = target_x.saturating_add(x);
                    if dst_x >= img_w {
                        continue;
                    }
                    let wm_idx = ((y as usize) * (wm_w as usize) + (x as usize)) * 4;
                    let wm_a = (wm_buf[wm_idx + 3] as f32 / 255.0) * opacity;

                    if wm_a > 0.001 {
                        let px = rgb16.get_pixel_mut(dst_x, dst_y);
                        // Exact 8->16 bit expansion: 255 * 257 = 65535
                        let wm_r = u16::from(wm_buf[wm_idx]) * 257;
                        let wm_g = u16::from(wm_buf[wm_idx + 1]) * 257;
                        let wm_b = u16::from(wm_buf[wm_idx + 2]) * 257;

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
    fn test_watermark_extended_glyphs_and_empty_shutter() {
        for ch in ['©', '@', '_', ',', 'Ç', 'Ğ', 'İ', 'Ö', 'Ş', 'Ü', 'ç', 'ğ', 'ı', 'ö', 'ş', 'ü'] {
            let g = get_glyph_bitmap(ch);
            assert!(g.iter().any(|&row| row != 0), "Glyph for '{ch}' must not be blank");
        }

        let meta = RawMetadata {
            width: 100,
            height: 100,
            raw_width: 100,
            raw_height: 100,
            make: "Fujifilm".to_string(),
            model: "X-T5".to_string(),
            lens: "XF 35mm".to_string(),
            iso: 400.0,
            shutter: 0.0,
            aperture: 2.0,
            focal_length: 35.0,
            timestamp: 0,
            cam_mul: [1.0; 4],
        };
        assert_eq!(resolve_watermark_text("{shutter}", Some(&meta)), "");
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
    fn test_apply_watermark_to_rgb16_reaches_full_white_65535() {
        let mut img = DynamicImage::new_rgb16(200, 200);
        let opts = WatermarkOptions {
            enabled: true,
            watermark_type: "text".to_string(),
            text: "TEST".to_string(),
            logo_path: None,
            position_index: 8,
            opacity: 1.0,
            size: 14,
            margin: 10,
            color: "#ffffff".to_string(),
            drop_shadow: false,
        };

        apply_watermark(&mut img, &opts, None);
        let rgb16 = img.as_rgb16().unwrap();
        let mut max_val = 0u16;
        for y in 100..200 {
            for x in 100..200 {
                let px = rgb16.get_pixel(x, y);
                max_val = max_val.max(px[0]).max(px[1]).max(px[2]);
            }
        }
        assert_eq!(max_val, 65535, "Pure white watermark on 16-bit image must reach 65535");
    }

    #[test]
    fn test_watermark_handles_extreme_size_without_overflow() {
        let mut img = DynamicImage::new_rgb8(100, 100);
        let opts = WatermarkOptions {
            enabled: true,
            watermark_type: "text".to_string(),
            text: "©".repeat(10_000),
            logo_path: None,
            position_index: 4,
            opacity: 1.0,
            size: u32::MAX,
            margin: 8,
            color: "#ffffff".to_string(),
            drop_shadow: true,
        };
        apply_watermark(&mut img, &opts, None);
    }
}
