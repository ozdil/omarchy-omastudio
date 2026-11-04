//! The adjustment layers (local masks) packed for the GPU layers kernel in
//! `local.wgsl`, which mirrors [`crate::pipeline::layers::apply_layers_pixel`].
//!
//! Layout, all f32 (counts and offsets are whole numbers, exact below 2^24):
//! a 16-word header (word 0 the layer count, words 4..9 the BT.709 curve's parts),
//! then per enabled layer a 16-word block (shape count, invert, opacity, exposure,
//! contrast, highlights, shadows, whites, blacks, saturation, tint r/g/b) followed by
//! 16 words per shape (kind, mode, invert, six parameters, and for an image its
//! offset, width and height), then the mask images' values (0..255).

use crate::pipeline::layers::{self, AdjustmentLayer, MaskMode, MaskShape};
use crate::pipeline::transfer::Transfer;

pub const HEADER: usize = 16;
pub const BLOCK: usize = 16;

/// Shape kinds; must match the shader.
const LINEAR: f32 = 0.0;
const RADIAL: f32 = 1.0;
const LUMA: f32 = 2.0;
const IMAGE: f32 = 3.0;
const EMPTY: f32 = 4.0;
const COLOUR: f32 = 5.0;

/// Largest buffer this packs, in words (offsets must stay exact in f32).
const MAX_WORDS: usize = 1 << 24;

/// The layers packed for the shader; None when there are too many mask pixels.
pub fn pack(list: &[AdjustmentLayer]) -> Option<Vec<f32>> {
    let active: Vec<&AdjustmentLayer> = list.iter().filter(|l| l.enabled && l.opacity > 1e-4).collect();
    let shapes: usize = active.iter().map(|l| 1 + l.components.len()).sum();
    let mut out = vec![0.0f32; HEADER + BLOCK * (active.len() + shapes)];
    out[0] = active.len() as f32;
    out[4..9].copy_from_slice(&Transfer::bt709().parts());
    let mut images: Vec<f32> = Vec::new();
    let image_base = out.len();
    let mut at = HEADER;
    for layer in active {
        let block = [
            (1 + layer.components.len()) as f32,
            layer.invert as u8 as f32,
            layer.opacity,
            layer.exposure,
            layer.contrast,
            layer.highlights,
            layer.shadows,
            layer.whites,
            layer.blacks,
            layer.saturation,
            layer.tint_r,
            layer.tint_g,
            layer.tint_b,
        ];
        out[at..at + block.len()].copy_from_slice(&block);
        at += BLOCK;
        let parts = std::iter::once((MaskMode::Add, false, &layer.shape)).chain(layer.components.iter().map(|c| (c.mode, c.invert, &c.shape)));
        for (mode, invert, shape) in parts {
            let s = &mut out[at..at + BLOCK];
            s[1] = match mode {
                MaskMode::Add => 0.0,
                MaskMode::Subtract => 1.0,
                MaskMode::Intersect => 2.0,
            };
            s[2] = invert as u8 as f32;
            let p: &mut [f32] = &mut [0.0; 6];
            s[0] = match shape {
                MaskShape::LinearGradient { start_x, start_y, end_x, end_y } => {
                    p[..4].copy_from_slice(&[*start_x, *start_y, *end_x, *end_y]);
                    LINEAR
                }
                MaskShape::RadialGradient { center_x, center_y, radius_x, radius_y, feather } => {
                    p[..5].copy_from_slice(&[*center_x, *center_y, *radius_x, *radius_y, *feather]);
                    RADIAL
                }
                MaskShape::LumaRange { min_luma, max_luma, falloff } => {
                    p[..3].copy_from_slice(&[*min_luma, *max_luma, *falloff]);
                    LUMA
                }
                MaskShape::ColorRange { r, g, b, range, feather } => {
                    let t = layers::oklab_of(*r, *g, *b);
                    p[..5].copy_from_slice(&[t[0], t[1], t[2], *range, *feather]);
                    COLOUR
                }
                MaskShape::Bitmap { .. } | MaskShape::Brush { .. } => match layers::shape_bitmap(shape) {
                    Some(m) => {
                        s[9] = (image_base + images.len()) as f32;
                        s[10] = m.width as f32;
                        s[11] = m.height as f32;
                        images.extend(m.data.iter().map(|&v| v as f32));
                        IMAGE
                    }
                    None => EMPTY,
                },
            };
            s[3..9].copy_from_slice(p);
            at += BLOCK;
        }
        if image_base + images.len() > MAX_WORDS {
            return None;
        }
    }
    out.extend(images);
    Some(out)
}
