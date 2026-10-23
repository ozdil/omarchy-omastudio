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

/// Long edge, in pixels, of the copy the edges are measured on
const ANALYSIS_EDGE: usize = 1000;

/// Paints [`PEAKING_RGB`] over the parts of `rgb` (`width` × `height`, 3 channels)
/// whose local contrast is above the threshold. Soft or out-of-focus edges spread
/// their contrast over several pixels and stay below it.
///
/// Edges are measured on a copy reduced to about [`ANALYSIS_EDGE`] pixels, roughly
/// what the window shows of the whole photo. At that size sensor noise at high ISO
/// and fine fabric prints average out while eyelashes and other real detail stay,
/// and each mark covers enough of the view to stay visible when it is scaled down.
/// The threshold follows the photo: a portrait at a wide aperture has gentler edges
/// than a detailed landscape, so its sharpest few percent still show even when none
/// reaches [`THRESHOLD`].
pub fn apply(rgb: &mut [u8], width: usize, height: usize) {
    if width < 3 || height < 3 || rgb.len() < width * height * 3 {
        return;
    }
    let k = width.max(height).div_ceil(ANALYSIS_EDGE).max(1);
    let (w, h) = (width / k, height / k);
    if w < 3 || h < 3 {
        return;
    }
    // Box-averaged luma of each k × k block
    let luma: Vec<i32> = (0..h)
        .into_par_iter()
        .flat_map_iter(|by| {
            let rgb = &*rgb;
            (0..w).map(move |bx| {
                let mut sum = 0u32;
                for y in by * k..(by + 1) * k {
                    let row = &rgb[(y * width + bx * k) * 3..(y * width + (bx + 1) * k) * 3];
                    for p in row.chunks_exact(3) {
                        sum += (p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) >> 8;
                    }
                }
                (sum / (k * k) as u32) as i32
            })
        })
        .collect();
    let laplacian = |i: usize| (4 * luma[i] - luma[i - 1] - luma[i + 1] - luma[i - w] - luma[i + w]).abs();
    let mut histogram = vec![0u64; 1021];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            histogram[laplacian(y * w + x).min(1020) as usize] += 1;
        }
    }
    let threshold = adaptive_threshold(&histogram);
    rgb.par_chunks_mut(width * 3)
        .take(h * k)
        .enumerate()
        .filter(|(y, _)| *y / k > 0 && *y / k + 1 < h)
        .for_each(|(y, row)| {
            let by = y / k;
            for bx in 1..w - 1 {
                if laplacian(by * w + bx) > threshold {
                    for x in bx * k..(bx + 1) * k {
                        row[x * 3..x * 3 + 3].copy_from_slice(&PEAKING_RGB);
                    }
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
    fn noise_in_a_large_photo_is_not_taken_for_focus() {
        // High-ISO grain on a flat wall, with one sharp edge in the middle
        let (w, h) = (2000, 400);
        let noise = |x: usize, y: usize| if (x * 7 + y * 13) % 3 == 0 { 40 } else { 0 };
        let mut photo = image(w, h, |x, y| (if x < w / 2 { 60 } else { 180 }) + noise(x, y) as u8);
        apply(&mut photo, w, h);
        let marked_columns: Vec<usize> = (0..w).filter(|&x| photo[(h / 2 * w + x) * 3..][..3] == PEAKING_RGB).collect();
        assert!(!marked_columns.is_empty());
        assert!(marked_columns.iter().all(|&x| x.abs_diff(w / 2) <= 4), "{marked_columns:?}");
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
