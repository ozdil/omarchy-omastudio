//! Focus peaking: marks the edges sharp enough to be in focus on a rendered 8-bit
//! RGB view, so the plane of focus shows at a glance.

use rayon::prelude::*;

/// Colour painted on in-focus edges (a green that photographs rarely contain and that
/// differs from the clipping and gamut warnings)
pub const PEAKING_RGB: [u8; 3] = [40, 255, 90];

/// Local contrast (4-neighbour Laplacian of display luma, 0..1020) above which an
/// edge counts as in focus
const THRESHOLD: i32 = 110;

/// Paints [`PEAKING_RGB`] over the pixels of `rgb` (`width` × `height`, 3 channels)
/// whose local contrast is above the threshold. Soft or out-of-focus edges spread
/// their contrast over several pixels and stay below it.
pub fn apply(rgb: &mut [u8], width: usize, height: usize) {
    if width < 3 || height < 3 || rgb.len() < width * height * 3 {
        return;
    }
    let luma: Vec<i32> = rgb
        .par_chunks(3)
        .take(width * height)
        .map(|p| (p[0] as i32 * 54 + p[1] as i32 * 183 + p[2] as i32 * 19) >> 8)
        .collect();
    rgb.par_chunks_mut(width * 3)
        .take(height)
        .enumerate()
        .filter(|(y, _)| *y > 0 && *y + 1 < height)
        .for_each(|(y, row)| {
            for x in 1..width - 1 {
                let i = y * width + x;
                let lap = 4 * luma[i] - luma[i - 1] - luma[i + 1] - luma[i - width] - luma[i + width];
                if lap.abs() > THRESHOLD {
                    row[x * 3..x * 3 + 3].copy_from_slice(&PEAKING_RGB);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: usize, height: usize, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                out.extend_from_slice(&[f(x, y); 3]);
            }
        }
        out
    }

    fn marked(rgb: &[u8]) -> usize {
        rgb.chunks(3).filter(|p| *p == PEAKING_RGB).count()
    }

    #[test]
    fn a_hard_edge_is_marked_and_a_soft_one_is_not() {
        let (w, h) = (32, 8);
        let mut sharp = image(w, h, |x, _| if x < 16 { 20 } else { 230 });
        apply(&mut sharp, w, h);
        assert!(marked(&sharp) >= 2 * (h - 2));

        // The same step spread over 16 pixels, as a defocused edge is
        let mut soft = image(w, h, |x, _| (20 + (x.clamp(8, 24) - 8) * 210 / 16) as u8);
        apply(&mut soft, w, h);
        assert_eq!(marked(&soft), 0);
    }

    #[test]
    fn flat_areas_and_tiny_images_are_left_alone() {
        let mut flat = image(16, 16, |_, _| 128);
        let before = flat.clone();
        apply(&mut flat, 16, 16);
        assert_eq!(flat, before);
        let mut tiny = vec![0u8, 255, 0, 255, 0, 255];
        apply(&mut tiny, 2, 1);
        assert_eq!(tiny, vec![0u8, 255, 0, 255, 0, 255]);
    }
}
