#![allow(clippy::too_many_arguments)]
use rayon::prelude::*;

/// Applies radial vignetting effect
#[inline(always)]
pub fn apply_vignette(
    r: f32,
    g: f32,
    b: f32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    vignette_amount: f32,
) -> (f32, f32, f32) {
    if vignette_amount.abs() < 0.01 {
        return (r, g, b);
    }

    let cx = width as f32 / 2.0;
    let cy = height as f32 / 2.0;
    let max_dist = (cx * cx + cy * cy).sqrt();

    let dx = x as f32 - cx;
    let dy = y as f32 - cy;
    let dist = (dx * dx + dy * dy).sqrt() / max_dist;

    // Smooth cosine falloff from 0.4 radius to outer edge
    let falloff = ((dist - 0.4) / 0.6).clamp(0.0, 1.0);
    let smooth = falloff * falloff * (3.0 - 2.0 * falloff);

    let factor = 1.0 + (vignette_amount / 100.0) * smooth;
    let clamped_factor = factor.max(0.0);

    (r * clamped_factor, g * clamped_factor, b * clamped_factor)
}

/// Reduces chromatic aberration fringing (purple and green edges) of one pixel.
/// `edge` (0..1) says how close the pixel is to a high-contrast edge; fringes only
/// occur there, so purple flowers and green leaves away from edges keep their colour.
#[inline(always)]
pub fn apply_defringe(r: f32, g: f32, b: f32, amount: f32, edge: f32) -> (f32, f32, f32) {
    if amount <= 0.01 || edge <= 0.001 {
        return (r, g, b);
    }
    let factor = (amount / 100.0).clamp(0.0, 1.0) * edge.clamp(0.0, 1.0);
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

    // Detect purple fringing: R and B much higher than G
    let purple = (r.min(b) - g).max(0.0);
    // Detect green fringing: G much higher than (R + B) / 2
    let green = (g - (r + b) * 0.5).max(0.0);

    let fringe = (purple + green).clamp(0.0, 1.0) * factor * 0.6;
    if fringe > 0.001 {
        let nr = r * (1.0 - fringe) + luma * fringe;
        let ng = g * (1.0 - fringe) + luma * fringe;
        let nb = b * (1.0 - fringe) + luma * fringe;
        return (nr, ng, nb);
    }
    (r, g, b)
}

/// Defringe over a buffer: the strength follows the local luminance contrast,
/// spread a few pixels (`scale`d to the frame) to reach the whole fringe beside an edge.
pub fn apply_defringe_buffer(rgb_buffer: &mut [f32], width: usize, height: usize, amount: f32, scale: f32) {
    if amount <= 0.01 || width < 3 || height < 3 || rgb_buffer.len() < width * height * 3 {
        return;
    }
    let luma: Vec<f32> = rgb_buffer
        .par_chunks(3)
        .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
        .collect();
    let mut grad = vec![0.0f32; width * height];
    grad.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let (yp, yn) = (y.saturating_sub(1), (y + 1).min(height - 1));
        for (x, g) in row.iter_mut().enumerate() {
            let (xp, xn) = (x.saturating_sub(1), (x + 1).min(width - 1));
            let gx = (luma[y * width + xn] - luma[y * width + xp]).abs();
            let gy = (luma[yn * width + x] - luma[yp * width + x]).abs();
            *g = gx.max(gy);
        }
    });
    let spread = crate::pipeline::presence::fast_separable_blur(&grad, width, height, crate::pipeline::presence::scaled_radius(2, scale));
    rgb_buffer.par_chunks_mut(3).zip(spread.par_iter()).for_each(|(p, &e)| {
        let edge = ((e - 0.01) / 0.05).clamp(0.0, 1.0);
        let (r, g, b) = apply_defringe(p[0], p[1], p[2], amount, edge);
        p[0] = r;
        p[1] = g;
        p[2] = b;
    });
}

/// Applies high-precision multi-threaded Detail enhancements:
/// - **Sharpness**: High-frequency edge enhancement with noise-gate thresholding.
/// - **Luma Denoise**: Bilateral edge-preserving filter suppressing grain while locking sharp contours.
/// - **Chroma Denoise**: Low-frequency chrominance smoothing wiping out shadow color blotches without loss of sharpness.
pub fn apply_detail(
    rgb_buffer: &mut [f32],
    width: usize,
    height: usize,
    sharpness: f32,
    denoise_lum: f32,
    denoise_col: f32,
) {
    apply_detail_scaled(rgb_buffer, width, height, sharpness, denoise_lum, denoise_col, 1.0);
}

/// [`apply_detail`] with the colour noise reduction's neighbourhood scaled to the
/// frame (see [`crate::pipeline::presence::apply_presence_scaled`]). Sharpening and
/// luminance noise reduction work on single pixels at every size.
pub fn apply_detail_scaled(
    rgb_buffer: &mut [f32],
    width: usize,
    height: usize,
    sharpness: f32,
    denoise_lum: f32,
    denoise_col: f32,
    scale: f32,
) {
    if sharpness <= 0.1 && denoise_lum <= 0.1 && denoise_col <= 0.1 {
        return;
    }

    let num_pixels = width * height;
    if rgb_buffer.len() < num_pixels * 3 || width < 3 || height < 3 {
        return;
    }

    // Extract Luminance and Chrominance (Cr, Cb) channels
    let mut luma = vec![0.0f32; num_pixels];
    let mut cr = vec![0.0f32; num_pixels];
    let mut cb = vec![0.0f32; num_pixels];

    luma.par_iter_mut()
        .zip(cr.par_iter_mut())
        .zip(cb.par_iter_mut())
        .enumerate()
        .for_each(|(i, ((l, r_diff), b_diff))| {
            let px = i * 3;
            let y = 0.2126 * rgb_buffer[px] + 0.7152 * rgb_buffer[px + 1] + 0.0722 * rgb_buffer[px + 2];
            *l = y;
            *r_diff = rgb_buffer[px] - y;
            *b_diff = rgb_buffer[px + 2] - y;
        });

    let do_chroma_dn = denoise_col > 0.5;
    let do_luma_dn = denoise_lum > 0.5;
    let do_sharp = sharpness > 0.5;

    // 1. Chroma Denoise: Separable multi-pixel color average (eliminates color blotches with zero loss of sharpness)
    let (cr_smooth, cb_smooth) = if do_chroma_dn {
        let weight = (denoise_col / 100.0).clamp(0.0, 1.0);
        let radius = crate::pipeline::presence::scaled_radius(if denoise_col > 40.0 { 2 } else { 1 }, scale);
        let blur_cr = crate::pipeline::presence::fast_separable_blur(&cr, width, height, radius);
        let blur_cb = crate::pipeline::presence::fast_separable_blur(&cb, width, height, radius);

        let mut out_cr = vec![0.0f32; num_pixels];
        let mut out_cb = vec![0.0f32; num_pixels];

        out_cr.par_iter_mut().zip(out_cb.par_iter_mut()).enumerate().for_each(|(i, (dst_r, dst_b))| {
            *dst_r = cr[i] * (1.0 - weight) + blur_cr[i] * weight;
            *dst_b = cb[i] * (1.0 - weight) + blur_cb[i] * weight;
        });

        (Some(out_cr), Some(out_cb))
    } else {
        (None, None)
    };

    // 2. Luma Denoise & Sharpness on Luminance Channel
    let mut luma_out = vec![0.0f32; num_pixels];
    let dn_strength = if do_luma_dn { (denoise_lum / 100.0) * 0.75 } else { 0.0 };
    let sh_strength = if do_sharp { (sharpness / 100.0) * 0.80 } else { 0.0 };

    luma_out.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        let y_prev = if y > 0 { y - 1 } else { 0 };
        let y_next = if y + 1 < height { y + 1 } else { height - 1 };

        for x in 0..width {
            let x_prev = if x > 0 { x - 1 } else { 0 };
            let x_next = if x + 1 < width { x + 1 } else { width - 1 };

            let center = luma[y * width + x];
            let n_up = luma[y_prev * width + x];
            let n_dn = luma[y_next * width + x];
            let n_lt = luma[y * width + x_prev];
            let n_rt = luma[y * width + x_next];

            let mut final_l = center;

            // Bilateral-style noise reduction on flat areas
            if do_luma_dn {
                let sig = 0.06f32;
                let inv_sig = 1.0 / (2.0 * sig * sig);

                let w_up = (-((n_up - center) * (n_up - center)) * inv_sig).exp();
                let w_dn = (-((n_dn - center) * (n_dn - center)) * inv_sig).exp();
                let w_lt = (-((n_lt - center) * (n_lt - center)) * inv_sig).exp();
                let w_rt = (-((n_rt - center) * (n_rt - center)) * inv_sig).exp();
                let w_c = 1.0f32;

                let w_sum = w_up + w_dn + w_lt + w_rt + w_c;
                let blurred = (n_up * w_up + n_dn * w_dn + n_lt * w_lt + n_rt * w_rt + center * w_c) / w_sum;
                final_l = center * (1.0 - dn_strength) + blurred * dn_strength;
            }

            // Adaptive unsharp masking (boosts in-focus edges with noise gate)
            if do_sharp {
                let laplacian = 4.0 * center - (n_up + n_dn + n_lt + n_rt);
                let edge_mag = laplacian.abs();

                // Noise-gate threshold: do not sharpen tiny noise fluctuations (< 0.012)
                let noise_gate = ((edge_mag - 0.012) / 0.03).clamp(0.0, 1.0);
                let boost = laplacian * sh_strength * noise_gate;
                final_l = (final_l + boost).clamp(0.0, 1.5);
            }

            row[x] = final_l;
        }
    });

    // Reconstruct RGB with filtered luminance and chrominance
    rgb_buffer.par_chunks_mut(3).enumerate().for_each(|(i, rgb)| {
        let l = luma_out[i];
        let r_diff = if let Some(ref c) = cr_smooth { c[i] } else { cr[i] };
        let b_diff = if let Some(ref c) = cb_smooth { c[i] } else { cb[i] };

        // Since L = 0.2126*R + 0.7152*G + 0.0722*B
        // G = (L - 0.2126*(L + r_diff) - 0.0722*(L + b_diff)) / 0.7152
        let r = l + r_diff;
        let b = l + b_diff;
        let g = (l - 0.2126 * r - 0.0722 * b) / 0.7152;

        rgb[0] = r;
        rgb[1] = g;
        rgb[2] = b;
    });
}

/// Radial polynomial lens distortion: r_source = r * (1 + k1 * (r / max_r)^2).
/// When k1 > 0 the corners would sample outside the photo and come out black, so the
/// result is enlarged by 1 / (1 + k1) until the corners land on the photo's corners.
#[derive(Clone, Copy)]
pub struct LensDistortion {
    cx: f32,
    cy: f32,
    inv_max_r2: f32,
    k1: f32,
    fill: f32,
}

impl LensDistortion {
    /// None when `distortion` (-100..100) is too small to have an effect.
    pub fn new(width: usize, height: usize, distortion: f32) -> Option<Self> {
        if distortion.abs() < 0.1 || width == 0 || height == 0 {
            return None;
        }
        let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
        let k1 = (distortion / 100.0) * 0.15;
        Some(Self { cx, cy, inv_max_r2: 1.0 / (cx * cx + cy * cy), k1, fill: 1.0 / (1.0 + k1.max(0.0)) })
    }

    /// Where the output pixel (x, y) samples the source image.
    #[inline(always)]
    pub fn source(&self, x: f32, y: f32) -> (f32, f32) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let factor = (1.0 + self.k1 * (dx * dx + dy * dy) * self.inv_max_r2) * self.fill;
        (self.cx + dx * factor, self.cy + dy * factor)
    }
}

/// Largest tilt of the perspective correction, at a slider value of 100
const PERSPECTIVE_MAX_DEGREES: f32 = 30.0;
/// Focal length of the virtual camera for perspective correction, in half frame
/// diagonals (about a normal lens)
const PERSPECTIVE_FOCAL: f32 = 2.0;

/// The perspective correction as a homography in centred pixel coordinates: row-major
/// 3x3, mapping a point of the photo to where it lands in the corrected photo. The
/// photo is treated as a plane facing a virtual camera and tilted about its centre;
/// `vertical` and `horizontal` run -100..100 and `scale` is in percent. None when there
/// is nothing to correct.
pub fn perspective_matrix(width: usize, height: usize, vertical: f32, horizontal: f32, scale: f32) -> Option<[f32; 9]> {
    let finite = vertical.is_finite() && horizontal.is_finite() && scale.is_finite();
    let s = if finite { scale.clamp(50.0, 150.0) / 100.0 } else { 1.0 };
    if !finite || width == 0 || height == 0 || (vertical.abs() < 0.05 && horizontal.abs() < 0.05 && (s - 1.0).abs() < 1e-4) {
        return None;
    }
    let ax = (vertical.clamp(-100.0, 100.0) / 100.0 * PERSPECTIVE_MAX_DEGREES).to_radians();
    let ay = (horizontal.clamp(-100.0, 100.0) / 100.0 * PERSPECTIVE_MAX_DEGREES).to_radians();
    let f = PERSPECTIVE_FOCAL * ((width * width + height * height) as f32).sqrt() / 2.0;
    let (sx, cx) = ax.sin_cos();
    let (sy, cy) = ay.sin_cos();
    // R = Ry * Rx; the plane's points (x, y, 0) move to R (x, y, 0) + (0, 0, f) and are
    // projected with focal length f, so the centre stays where it is
    let (r11, r12) = (cy, sy * sx);
    let (r21, r22) = (0.0, cx);
    let (r31, r32) = (-sy, cy * sx);
    Some([s * r11, s * r12, 0.0, s * r21, s * r22, 0.0, r31 / f, r32 / f, 1.0])
}

/// Inverse of a 3x3 matrix (row-major); None when it is singular.
fn invert3(m: &[f32; 9]) -> Option<[f32; 9]> {
    let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6]) + m[2] * (m[3] * m[7] - m[4] * m[6]);
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    let d = 1.0 / det;
    Some([
        (m[4] * m[8] - m[5] * m[7]) * d,
        (m[2] * m[7] - m[1] * m[8]) * d,
        (m[1] * m[5] - m[2] * m[4]) * d,
        (m[5] * m[6] - m[3] * m[8]) * d,
        (m[0] * m[8] - m[2] * m[6]) * d,
        (m[2] * m[3] - m[0] * m[5]) * d,
        (m[3] * m[7] - m[4] * m[6]) * d,
        (m[1] * m[6] - m[0] * m[7]) * d,
        (m[0] * m[4] - m[1] * m[3]) * d,
    ])
}

/// The geometric corrections of the developed photo: lens distortion, then
/// perspective. `source` gives, for a pixel of the result, the position in the
/// photo it is sampled from.
#[derive(Clone, Copy)]
pub struct Geometry {
    lens: Option<LensDistortion>,
    /// Inverse perspective homography (output to photo), centred coordinates
    inverse: Option<[f32; 9]>,
    cx: f32,
    cy: f32,
}

impl Geometry {
    /// None when the recipe has no geometric correction.
    pub fn new(width: usize, height: usize, recipe: &crate::recipe::Recipe) -> Option<Self> {
        let lens = LensDistortion::new(width, height, recipe.lens_distortion);
        let inverse = perspective_matrix(width, height, recipe.perspective_vertical, recipe.perspective_horizontal, recipe.perspective_scale)
            .and_then(|m| invert3(&m));
        if lens.is_none() && inverse.is_none() {
            return None;
        }
        Some(Self { lens, inverse, cx: width as f32 / 2.0, cy: height as f32 / 2.0 })
    }

    /// Where the output pixel (x, y) samples the photo. Points the perspective sends
    /// behind the virtual camera come back far outside the photo.
    #[inline(always)]
    pub fn source(&self, x: f32, y: f32) -> (f32, f32) {
        let (mut x, mut y) = (x, y);
        if let Some(m) = &self.inverse {
            let (dx, dy) = (x - self.cx, y - self.cy);
            let w = m[6] * dx + m[7] * dy + m[8];
            if w <= 1e-6 {
                return (-1e9, -1e9);
            }
            x = (m[0] * dx + m[1] * dy + m[2]) / w + self.cx;
            y = (m[3] * dx + m[4] * dy + m[5]) / w + self.cy;
        }
        match &self.lens {
            Some(l) => l.source(x, y),
            None => (x, y),
        }
    }
}

/// Resamples the developed photo through `geometry` (lens distortion and perspective).
/// Samples outside the photo are black.
pub fn apply_geometry(input: &[f32], width: usize, height: usize, channels: usize, geometry: &Geometry) -> Vec<f32> {
    let mut output = vec![0.0f32; input.len()];
    output.par_chunks_mut(width * channels).enumerate().for_each(|(y_idx, row)| {
        for x_idx in 0..width {
            let (src_x, src_y) = geometry.source(x_idx as f32, y_idx as f32);
            let px = x_idx * channels;
            if src_x >= 0.0 && src_x < (width - 1) as f32 && src_y >= 0.0 && src_y < (height - 1) as f32 {
                let (x0, y0) = (src_x.floor() as usize, src_y.floor() as usize);
                let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
                let (fx, fy) = (src_x - x0 as f32, src_y - y0 as f32);
                for c in 0..channels.min(3) {
                    let at = |yy: usize, xx: usize| input[(yy * width + xx) * channels + c];
                    let top = at(y0, x0) * (1.0 - fx) + at(y0, x1) * fx;
                    let bottom = at(y1, x0) * (1.0 - fx) + at(y1, x1) * fx;
                    row[px + c] = top * (1.0 - fy) + bottom * fy;
                }
                if channels == 4 {
                    row[px + 3] = input[(y0 * width + x0) * channels + 3];
                }
            }
        }
    });
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Saturated colours away from edges keep their colour; a green fringe beside a
    /// hard edge is neutralised.
    #[test]
    fn defringe_only_touches_edges() {
        let (w, h) = (20usize, 20usize);
        let mut flat = vec![0.0f32; w * h * 3];
        for p in flat.chunks_mut(3) {
            p.copy_from_slice(&[0.15, 0.6, 0.12]);
        }
        let before = flat.clone();
        apply_defringe_buffer(&mut flat, w, h, 100.0, 1.0);
        assert_eq!(flat, before, "flat foliage must stay green");

        // Dark left half, bright right half, a green column on the boundary
        let mut edge = vec![0.0f32; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                let v = if x < 10 { [0.05, 0.05, 0.05] } else if x == 10 { [0.3, 0.7, 0.3] } else { [0.9, 0.9, 0.9] };
                edge[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&v);
            }
        }
        apply_defringe_buffer(&mut edge, w, h, 100.0, 1.0);
        let p = &edge[(10 * w + 10) * 3..(10 * w + 10) * 3 + 3];
        // The green excess was 0.4
        assert!(p[1] - (p[0] + p[2]) / 2.0 < 0.35, "fringe reduced: {p:?}");
    }

    #[test]
    fn perspective_keeps_the_centre_and_widens_the_top() {
        let (w, h) = (400usize, 300usize);
        let r = crate::recipe::Recipe { perspective_vertical: 50.0, ..Default::default() };
        let g = Geometry::new(w, h, &r).unwrap();
        let (sx, sy) = g.source(200.0, 150.0);
        assert!((sx - 200.0).abs() < 1e-3 && (sy - 150.0).abs() < 1e-3);
        // The top is enlarged, so an output point near the top-left corner samples
        // further in than one near the bottom-left corner
        let (tx, _) = g.source(20.0, 10.0);
        let (bx, _) = g.source(20.0, 290.0);
        assert!(tx > bx, "top {tx}, bottom {bx}");
        // Neutral settings are no correction at all
        assert!(Geometry::new(w, h, &crate::recipe::Recipe::default()).is_none());
    }

    #[test]
    fn perspective_inverse_undoes_the_forward_map() {
        let (w, h) = (640usize, 480usize);
        let m = perspective_matrix(w, h, -35.0, 60.0, 90.0).unwrap();
        let r = crate::recipe::Recipe { perspective_vertical: -35.0, perspective_horizontal: 60.0, perspective_scale: 90.0, ..Default::default() };
        let g = Geometry::new(w, h, &r).unwrap();
        for &(x, y) in &[(10.0f32, 20.0f32), (600.0, 30.0), (320.0, 400.0), (50.0, 470.0)] {
            let (dx, dy) = (x - 320.0, y - 240.0);
            let ww = m[6] * dx + m[7] * dy + m[8];
            let (ox, oy) = ((m[0] * dx + m[1] * dy + m[2]) / ww + 320.0, (m[3] * dx + m[4] * dy + m[5]) / ww + 240.0);
            let (bx, by) = g.source(ox, oy);
            assert!((bx - x).abs() < 0.01 && (by - y).abs() < 0.01, "({x}, {y}) -> ({ox}, {oy}) -> ({bx}, {by})");
        }
    }

    /// Positive correction does not leave black corners.
    #[test]
    fn pincushion_correction_fills_the_frame() {
        let lens = LensDistortion::new(400, 300, 50.0).unwrap();
        let (sx, sy) = lens.source(0.0, 0.0);
        assert!(sx >= -0.01 && sy >= -0.01, "corner samples ({sx}, {sy})");
        let (sx, sy) = lens.source(399.0, 299.0);
        assert!(sx <= 399.5 && sy <= 299.5);
    }
}
