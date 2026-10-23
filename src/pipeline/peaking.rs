//! Focus peaking: marks the edges sharp enough to be in focus on a rendered 8-bit
//! RGB view, so the plane of focus shows at a glance.

use rayon::prelude::*;

/// Colour painted on in-focus edges (a green that photographs rarely contain and that
/// differs from the clipping and gamut warnings)
pub const PEAKING_RGB: [u8; 3] = [40, 255, 90];

/// Local contrast (4-neighbour Laplacian of display luma, 0..1020) above which an
/// edge always counts as in focus
const THRESHOLD: i32 = 110;
/// Lowest threshold a soft photo can bring it down to; below it is noise and
/// defocused gradients
const MIN_THRESHOLD: i32 = 40;
/// Share of the photo's pixels that sets the threshold in a soft photo
const TOP_SHARE: f64 = 0.02;

/// Paints [`PEAKING_RGB`] over the pixels of `rgb` (`width` × `height`, 3 channels)
/// whose local contrast is above the threshold. Soft or out-of-focus edges spread
/// their contrast over several pixels and stay below it. The threshold follows the
/// photo: a portrait at a wide aperture or a photo shown fitted to the window has
/// gentler edges than a detailed landscape, so its sharpest few percent still show
/// even when none reaches [`THRESHOLD`].
pub fn apply(rgb: &mut [u8], width: usize, height: usize) {
    if width < 3 || height < 3 || rgb.len() < width * height * 3 {
        return;
    }
    let luma: Vec<i32> = rgb
        .par_chunks(3)
        .take(width * height)
        .map(|p| (p[0] as i32 * 54 + p[1] as i32 * 183 + p[2] as i32 * 19) >> 8)
        .collect();
    let laplacian = |i: usize| (4 * luma[i] - luma[i - 1] - luma[i + 1] - luma[i - width] - luma[i + width]).abs();
    let mut histogram = vec![0u64; 1021];
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            histogram[laplacian(y * width + x).min(1020) as usize] += 1;
        }
    }
    let threshold = adaptive_threshold(&histogram);
    rgb.par_chunks_mut(width * 3)
        .take(height)
        .enumerate()
        .filter(|(y, _)| *y > 0 && *y + 1 < height)
        .for_each(|(y, row)| {
            for x in 1..width - 1 {
                if laplacian(y * width + x) > threshold {
                    row[x * 3..x * 3 + 3].copy_from_slice(&PEAKING_RGB);
                }
            }
        });
}

/// The contrast of the photo's sharpest [`TOP_SHARE`] (with some margin), kept between
/// [`MIN_THRESHOLD`] and [`THRESHOLD`]
fn adaptive_threshold(histogram: &[u64]) -> i32 {
    let total: u64 = histogram.iter().sum();
    let wanted = (total as f64 * TOP_SHARE).ceil() as u64;
    let mut seen = 0;
    let mut level = 0;
    for (value, &count) in histogram.iter().enumerate().rev() {
        seen += count;
        if seen >= wanted {
            level = value as i32;
            break;
        }
    }
    (level * 4 / 5).clamp(MIN_THRESHOLD, THRESHOLD)
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
    fn a_gentle_but_sharp_edge_shows_in_a_soft_photo() {
        // A low-contrast step (a face against a background, seen fitted to the window)
        // stays under the fixed threshold but is the sharpest thing in the photo
        let (w, h) = (32, 8);
        let mut gentle = image(w, h, |x, _| if x < 16 { 100 } else { 160 });
        apply(&mut gentle, w, h);
        assert!(marked(&gentle) >= 2 * (h - 2));
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
