// Presence (texture, clarity, dehaze, midtone detail), detail (colour and luminance
// noise reduction, sharpening) and defringe on the GPU. Each entry point mirrors one
// loop of src/pipeline/presence.rs or src/pipeline/detail.rs; the Rust side chains
// them in the same order as the CPU.
//
// `pl` holds six planes of w*h floats:
//   0 luminance, 1 and 2 blur results or chroma, 3 luminance shift or blurred Cr,
//   4 scratch (the horizontal blur pass, the filtered luminance), 5 dark channel or
//   blurred Cb.

struct Params {
    w: u32,
    h: u32,
    src: u32,
    dst: u32,
    r: u32,
    mode: u32,
    flags: u32,
    pad: u32,
    f0: f32,
    f1: f32,
    f2: f32,
    f3: f32,
    f4: f32,
    f5: f32,
    f6: f32,
    f7: f32,
    // The geometry kernel's inverse homography and lens terms (Geometry::gpu_terms)
    g: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read_write> rgb: array<f32>;
@group(0) @binding(2) var<storage, read_write> pl: array<f32>;
// The display tail: final 8-bit pixels (R | G << 8 | B << 16), the same packed to
// 3 bytes per pixel, and the histogram and scope counts (layout in histogram.rs)
@group(0) @binding(3) var<storage, read_write> px8: array<u32>;
@group(0) @binding(4) var<storage, read_write> out8: array<u32>;
@group(0) @binding(5) var<storage, read_write> hist: array<atomic<u32>>;
// The adjustment layers, packed by gpu::layers::pack
@group(0) @binding(6) var<storage, read> lay: array<f32>;

const CHUNK: u32 = 32u;

fn plane(k: u32) -> u32 {
    return k * p.w * p.h;
}

fn luma_of(r: f32, g: f32, b: f32) -> f32 {
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

// Pixel index for the 16x16 per-pixel kernels, or w*h when outside the frame
fn pixel(id: vec3<u32>) -> u32 {
    if (id.x >= p.w || id.y >= p.h) {
        return p.w * p.h;
    }
    return id.y * p.w + id.x;
}

// Luminance into plane 0, zero into the shift plane 3
@compute @workgroup_size(16, 16, 1)
fn luma_init(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    pl[i] = luma_of(rgb[i * 3u], rgb[i * 3u + 1u], rgb[i * 3u + 2u]);
    pl[plane(3u) + i] = 0.0;
}

// Box blur along rows, clamped at the edges: each thread slides a window over CHUNK
// pixels of one row (presence::fast_separable_blur, horizontal pass)
@compute @workgroup_size(64, 1, 1)
fn blur_h(@builtin(global_invocation_id) id: vec3<u32>) {
    let y = id.y;
    let x0 = id.x * CHUNK;
    if (y >= p.h || x0 >= p.w) {
        return;
    }
    let r = i32(p.r);
    let last = i32(p.w) - 1;
    let sb = plane(p.src) + y * p.w;
    let db = plane(p.dst) + y * p.w;
    var sum = 0.0;
    for (var k = -r; k <= r; k++) {
        sum += pl[sb + u32(clamp(i32(x0) + k, 0, last))];
    }
    let inv = 1.0 / f32(2 * r + 1);
    let x1 = min(x0 + CHUNK, p.w);
    for (var x = x0; x < x1; x++) {
        pl[db + x] = sum * inv;
        let xi = i32(x);
        sum += pl[sb + u32(min(xi + r + 1, last))] - pl[sb + u32(max(xi - r, 0))];
    }
}

// The vertical pass: each thread slides down CHUNK rows of one column
@compute @workgroup_size(64, 1, 1)
fn blur_v(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x;
    let y0 = id.y * CHUNK;
    if (x >= p.w || y0 >= p.h) {
        return;
    }
    let r = i32(p.r);
    let last = i32(p.h) - 1;
    let sb = plane(p.src) + x;
    let db = plane(p.dst) + x;
    var sum = 0.0;
    for (var k = -r; k <= r; k++) {
        sum += pl[sb + u32(clamp(i32(y0) + k, 0, last)) * p.w];
    }
    let inv = 1.0 / f32(2 * r + 1);
    let y1 = min(y0 + CHUNK, p.h);
    for (var y = y0; y < y1; y++) {
        pl[db + y * p.w] = sum * inv;
        let yi = i32(y);
        sum += pl[sb + u32(min(yi + r + 1, last)) * p.w] - pl[sb + u32(max(yi - r, 0)) * p.w];
    }
}

// Adds one module's luminance shift to plane 3: mode 0 texture (plane 1 - plane 2),
// 1 clarity (luminance - plane 1, midtones), 2 midtone detail (plane 1 - plane 2, bell)
@compute @workgroup_size(16, 16, 1)
fn accum(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let l = pl[i];
    let a = pl[plane(1u) + i];
    let s = plane(3u) + i;
    if (p.mode == 0u) {
        let detail = a - pl[plane(2u) + i];
        pl[s] = pl[s] + detail * p.f0;
    } else if (p.mode == 1u) {
        let midtone_mask = clamp(4.0 * l * (1.0 - l), 0.0, 1.0);
        let local_contrast = l - a;
        pl[s] = pl[s] + local_contrast * p.f0 * midtone_mask;
    } else {
        let md_detail = a - pl[plane(2u) + i];
        let dist = clamp(l, 0.0, 1.0) - 0.5;
        let midtone_bell = exp(-dist * dist / 0.06);
        pl[s] = pl[s] + md_detail * p.f0 * midtone_bell;
    }
}

// Dark channel min(R, G, B) into plane 5
@compute @workgroup_size(16, 16, 1)
fn dark(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    pl[plane(5u) + i] = min(min(rgb[i * 3u], rgb[i * 3u + 1u]), rgb[i * 3u + 2u]);
}

// presence::apply_presence_scaled's final loop. flags bit 0: dehaze with the veil in
// plane 1 and factor f0
@compute @workgroup_size(16, 16, 1)
fn presence_apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    var r = rgb[i * 3u];
    var g = rgb[i * 3u + 1u];
    var b = rgb[i * 3u + 2u];
    let cur_luma = pl[i];
    let luma_shift = pl[plane(3u) + i];

    if ((p.flags & 1u) != 0u) {
        let v = clamp(pl[plane(1u) + i], 0.0, 0.90);
        let t = p.f0;
        if (t > 0.0) {
            let atmospheric_light = 0.88;
            let strength = t * 0.65;
            let restore = clamp(v * strength, 0.0, 0.5);
            let d = max(1.0 - restore, 0.1);
            r = (r - atmospheric_light * restore) / d;
            g = (g - atmospheric_light * restore) / d;
            b = (b - atmospheric_light * restore) / d;
            let new_luma = luma_of(r, g, b);
            let sat_boost = 1.0 + t * 0.35 * v;
            r = new_luma + (r - new_luma) * sat_boost;
            g = new_luma + (g - new_luma) * sat_boost;
            b = new_luma + (b - new_luma) * sat_boost;
        } else {
            let mist = (-t * 0.40) * (1.0 - v * 0.5);
            r = r + (0.85 - r) * mist;
            g = g + (0.85 - g) * mist;
            b = b + (0.90 - b) * mist;
        }
    }

    if (abs(luma_shift) > 1e-5) {
        let new_l = max(cur_luma + luma_shift, 0.0);
        r = new_l + (r - cur_luma);
        g = new_l + (g - cur_luma);
        b = new_l + (b - cur_luma);
    }
    rgb[i * 3u] = r;
    rgb[i * 3u + 1u] = g;
    rgb[i * 3u + 2u] = b;
}

// detail::apply_detail_scaled's split: luminance to plane 0, R - Y to 1, B - Y to 2
@compute @workgroup_size(16, 16, 1)
fn detail_extract(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let r = rgb[i * 3u];
    let b = rgb[i * 3u + 2u];
    let y = luma_of(r, rgb[i * 3u + 1u], b);
    pl[i] = y;
    pl[plane(1u) + i] = r - y;
    pl[plane(2u) + i] = b - y;
}

// Luminance noise reduction and sharpening from plane 0 into plane 4. flags bit 0
// noise reduction (strength f0), bit 1 sharpening (strength f1)
@compute @workgroup_size(16, 16, 1)
fn luma_filter(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let x = id.x;
    let y = id.y;
    let yp = select(0u, y - 1u, y > 0u);
    let yn = select(p.h - 1u, y + 1u, y + 1u < p.h);
    let xp = select(0u, x - 1u, x > 0u);
    let xn = select(p.w - 1u, x + 1u, x + 1u < p.w);

    let center = pl[i];
    let n_up = pl[yp * p.w + x];
    let n_dn = pl[yn * p.w + x];
    let n_lt = pl[y * p.w + xp];
    let n_rt = pl[y * p.w + xn];
    var final_l = center;

    if ((p.flags & 1u) != 0u) {
        let sig = 0.06;
        let inv_sig = 1.0 / (2.0 * sig * sig);
        let w_up = exp(-((n_up - center) * (n_up - center)) * inv_sig);
        let w_dn = exp(-((n_dn - center) * (n_dn - center)) * inv_sig);
        let w_lt = exp(-((n_lt - center) * (n_lt - center)) * inv_sig);
        let w_rt = exp(-((n_rt - center) * (n_rt - center)) * inv_sig);
        let w_sum = w_up + w_dn + w_lt + w_rt + 1.0;
        let blurred = (n_up * w_up + n_dn * w_dn + n_lt * w_lt + n_rt * w_rt + center) / w_sum;
        final_l = center * (1.0 - p.f0) + blurred * p.f0;
    }
    if ((p.flags & 2u) != 0u) {
        let laplacian = 4.0 * center - (n_up + n_dn + n_lt + n_rt);
        let noise_gate = clamp((abs(laplacian) - 0.012) / 0.03, 0.0, 1.0);
        final_l = clamp(final_l + laplacian * p.f1 * noise_gate, 0.0, 1.5);
    }
    pl[plane(4u) + i] = final_l;
}

// RGB from the filtered luminance (plane 4) and chroma; flags bit 0 blends in the
// blurred chroma (planes 3 and 5) by f0
@compute @workgroup_size(16, 16, 1)
fn reconstruct(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let l = pl[plane(4u) + i];
    var r_diff = pl[plane(1u) + i];
    var b_diff = pl[plane(2u) + i];
    if ((p.flags & 1u) != 0u) {
        r_diff = r_diff * (1.0 - p.f0) + pl[plane(3u) + i] * p.f0;
        b_diff = b_diff * (1.0 - p.f0) + pl[plane(5u) + i] * p.f0;
    }
    let r = l + r_diff;
    let b = l + b_diff;
    rgb[i * 3u] = r;
    rgb[i * 3u + 1u] = (l - 0.2126 * r - 0.0722 * b) / 0.7152;
    rgb[i * 3u + 2u] = b;
}

// Defringe's edge strength: the larger luminance gradient, into plane 1
@compute @workgroup_size(16, 16, 1)
fn gradient(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let x = id.x;
    let y = id.y;
    let yp = select(0u, y - 1u, y > 0u);
    let yn = min(y + 1u, p.h - 1u);
    let xp = select(0u, x - 1u, x > 0u);
    let xn = min(x + 1u, p.w - 1u);
    let gx = abs(pl[y * p.w + xn] - pl[y * p.w + xp]);
    let gy = abs(pl[yn * p.w + x] - pl[yp * p.w + x]);
    pl[plane(1u) + i] = max(gx, gy);
}

// detail::apply_defringe with the spread edge strength in plane 2 and amount f0
@compute @workgroup_size(16, 16, 1)
fn defringe_apply(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let edge = clamp((pl[plane(2u) + i] - 0.01) / 0.05, 0.0, 1.0);
    if (edge <= 0.001) {
        return;
    }
    let r = rgb[i * 3u];
    let g = rgb[i * 3u + 1u];
    let b = rgb[i * 3u + 2u];
    let factor = clamp(p.f0 / 100.0, 0.0, 1.0) * edge;
    let luma = luma_of(r, g, b);
    let purple = max(min(r, b) - g, 0.0);
    let green = max(g - (r + b) * 0.5, 0.0);
    let fringe = clamp(purple + green, 0.0, 1.0) * factor * 0.6;
    if (fringe > 0.001) {
        rgb[i * 3u] = r * (1.0 - fringe) + luma * fringe;
        rgb[i * 3u + 1u] = g * (1.0 - fringe) + luma * fringe;
        rgb[i * 3u + 2u] = b * (1.0 - fringe) + luma * fringe;
    }
}

// ---- Display tail: grain, vignette, 8-bit output, histogram and scopes ----

// film_grain::hash_noise
fn hash_noise(x: u32, y: u32, seed: u32) -> f32 {
    var n = (x * 374761393u) ^ (y * 668265263u) ^ (seed * 314159265u);
    n = (n ^ (n >> 13u)) * 1274126177u;
    let f = f32(n & 0x007FFFFFu) / 8388607.0;
    return f * 2.0 - 1.0;
}

// Grain (flags bit 0: f0 amount, f1 step, f2 fine step, f3 roughness weight), vignette
// (bit 1: f4 amount) and the 8-bit quantization of pixel (x, y), as
// process_buffer_16_to_8_ex does on the CPU. With geometry it runs twice: mode 1 adds
// the grain in place, mode 2 takes the resampled frame from the planes.
@compute @workgroup_size(16, 16, 1)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    // Mode 2 reads the geometry kernel's result from the planes
    var r = rgb[i * 3u];
    var g = rgb[i * 3u + 1u];
    var b = rgb[i * 3u + 2u];
    if (p.mode == 2u) {
        r = pl[i * 3u];
        g = pl[i * 3u + 1u];
        b = pl[i * 3u + 2u];
    }
    let x = id.x;
    let y = id.y;
    if ((p.flags & 1u) != 0u) {
        let sample_y = u32(f32(y) / p.f1);
        let fine_y = u32(f32(y) / p.f2);
        let sample_x = u32(f32(x) / p.f1);
        let luma = clamp(luma_of(r, g, b), 0.0, 1.0);
        let base = 4.0 * luma * (1.0 - luma);
        var response = 0.0;
        if (base > 0.0) {
            response = pow(base, 1.15);
        }
        let n1 = hash_noise(sample_x, sample_y, 42u);
        let n2 = hash_noise(u32(f32(x) / p.f2), fine_y, 1337u);
        let combined = n1 * (1.0 - p.f3 * 0.4) + n2 * (p.f3 * 0.4);
        let delta = combined * p.f0 * response;
        r = max(r + delta, 0.0);
        g = max(g + delta, 0.0);
        b = max(b + delta, 0.0);
    }
    // Mode 1 stops after the grain: the geometry resamples the grained frame
    if (p.mode == 1u) {
        rgb[i * 3u] = r;
        rgb[i * 3u + 1u] = g;
        rgb[i * 3u + 2u] = b;
        return;
    }
    if ((p.flags & 2u) != 0u) {
        let cx = f32(p.w) / 2.0;
        let cy = f32(p.h) / 2.0;
        let max_dist = sqrt(cx * cx + cy * cy);
        let dx = f32(x) - cx;
        let dy = f32(y) - cy;
        let dist = sqrt(dx * dx + dy * dy) / max_dist;
        let falloff = clamp((dist - 0.4) / 0.6, 0.0, 1.0);
        let smoothed = falloff * falloff * (3.0 - 2.0 * falloff);
        let factor = max(1.0 + (p.f4 / 100.0) * smoothed, 0.0);
        r = r * factor;
        g = g * factor;
        b = b * factor;
    }
    let r8 = u32(clamp(r, 0.0, 1.0) * 255.0);
    let g8 = u32(clamp(g, 0.0, 1.0) * 255.0);
    let b8 = u32(clamp(b, 0.0, 1.0) * 255.0);
    px8[i] = r8 | (g8 << 8u) | (b8 << 16u);
}

// --- Adjustment layers (pipeline::layers::apply_layers_pixel) -----------------------

fn lsmooth(t: f32) -> f32 {
    let c = clamp(t, 0.0, 1.0);
    return c * c * (3.0 - 2.0 * c);
}

fn l_encode(lin: f32) -> f32 {
    if (!(lin > 0.0)) { return 0.0; }
    if (lin < lay[7]) { return lin * lay[5]; }
    return pow(lin, lay[4]) * (1.0 + lay[8]) - lay[8];
}

fn l_decode(enc: f32) -> f32 {
    if (!(enc > 0.0)) { return 0.0; }
    if (enc < lay[6]) { return enc / lay[5]; }
    return pow((enc + lay[8]) / (1.0 + lay[8]), 1.0 / lay[4]);
}

fn l_cbrt(v: f32) -> f32 {
    return sign(v) * pow(abs(v), 1.0 / 3.0);
}

fn l_oklab(c: vec3<f32>) -> vec3<f32> {
    let r = pow(max(c.x, 0.0), 2.2);
    let g = pow(max(c.y, 0.0), 2.2);
    let b = pow(max(c.z, 0.0), 2.2);
    let l = l_cbrt(0.41222146 * r + 0.53633255 * g + 0.051445995 * b);
    let m = l_cbrt(0.2119035 * r + 0.6806995 * g + 0.10739696 * b);
    let s = l_cbrt(0.08830246 * r + 0.28171885 * g + 0.6299787 * b);
    return vec3<f32>(
        0.21045426 * l + 0.7936178 * m - 0.004072047 * s,
        1.9779985 * l - 2.4285922 * m + 0.4505937 * s,
        0.025904037 * l + 0.78277177 * m - 0.80867577 * s,
    );
}

fn l_image(at: u32, u: f32, v: f32) -> f32 {
    let base = u32(lay[at + 9u]);
    let iw = u32(lay[at + 10u]);
    let ih = u32(lay[at + 11u]);
    let x = max(clamp(u, 0.0, 1.0) * f32(iw - 1u), 0.0);
    let y = max(clamp(v, 0.0, 1.0) * f32(ih - 1u), 0.0);
    let x0 = u32(x);
    let y0 = u32(y);
    let x1 = min(x0 + 1u, iw - 1u);
    let y1 = min(y0 + 1u, ih - 1u);
    let fx = x - f32(x0);
    let fy = y - f32(y0);
    let a = lay[base + y0 * iw + x0];
    let b = lay[base + y0 * iw + x1];
    let c = lay[base + y1 * iw + x0];
    let d = lay[base + y1 * iw + x1];
    let top = a + (b - a) * fx;
    let bottom = c + (d - c) * fx;
    return (top + (bottom - top) * fy) / 255.0;
}

// The weight of the shape at word `at` (layers::shape_weight)
fn l_shape(at: u32, u: f32, v: f32, c: vec3<f32>) -> f32 {
    let kind = u32(lay[at]);
    let p0 = lay[at + 3u];
    let p1 = lay[at + 4u];
    let p2 = lay[at + 5u];
    let p3 = lay[at + 6u];
    let p4 = lay[at + 7u];
    if (kind == 0u) {
        let dx = p2 - p0;
        let dy = p3 - p1;
        let len_sq = dx * dx + dy * dy;
        if (len_sq <= 1e-6) { return 1.0; }
        let proj = ((u - p0) * dx + (v - p1) * dy) / len_sq;
        return lsmooth(proj);
    }
    if (kind == 1u) {
        let f = clamp(p4, 0.01, 1.0);
        let nx = (u - p0) / max(p2, 1e-4);
        let ny = (v - p1) / max(p3, 1e-4);
        let dist = sqrt(nx * nx + ny * ny);
        if (dist >= 1.0) { return 0.0; }
        if (dist <= 1.0 - f) { return 1.0; }
        return lsmooth((1.0 - dist) / f);
    }
    if (kind == 2u) {
        let luma = 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z;
        let f = clamp(p2, 0.001, 0.5);
        let min_in = max(p0 - f, 0.0);
        let max_in = min(p1 + f, 1.0);
        if (luma < min_in || luma > max_in) { return 0.0; }
        if (luma >= p0 && luma <= p1) { return 1.0; }
        if (luma < p0) { return lsmooth((luma - min_in) / f); }
        return lsmooth((max_in - luma) / f);
    }
    if (kind == 3u) {
        return l_image(at, u, v);
    }
    if (kind == 5u) {
        let q = l_oklab(c);
        let dl = q.x - p0;
        let da = q.y - p1;
        let db = q.z - p2;
        let d = sqrt(0.5 * dl * dl + da * da + db * db);
        let inner = clamp(p3, 0.0, 1.0) * 0.3;
        let soft = clamp(p4, 0.0, 1.0) * 0.3;
        if (d <= inner) { return 1.0; }
        if (soft <= 1e-6) { return 0.0; }
        return lsmooth(1.0 - (d - inner) / soft);
    }
    return 0.0;
}

// Applies the layers to rgb in place. p.f0 and p.f1 are 1 / frame width and height,
// p.r and p.mode the band's offset in the frame.
@compute @workgroup_size(16, 16, 1)
fn layers(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let u = (f32(p.r + id.x) + 0.5) * p.f0;
    let v = (f32(p.mode + id.y) + 0.5) * p.f1;
    var c = vec3<f32>(rgb[i * 3u], rgb[i * 3u + 1u], rgb[i * 3u + 2u]);
    let count = u32(lay[0]);
    var at = 16u;
    for (var n = 0u; n < count; n++) {
        let shapes = u32(lay[at]);
        let invert = lay[at + 1u] > 0.5;
        let opacity = lay[at + 2u];
        let exposure = lay[at + 3u];
        let contrast = lay[at + 4u];
        let highlights = lay[at + 5u];
        let shadows = lay[at + 6u];
        let whites = lay[at + 7u];
        let blacks = lay[at + 8u];
        let saturation = lay[at + 9u];
        let tint = vec3<f32>(lay[at + 10u], lay[at + 11u], lay[at + 12u]);
        var s = at + 16u;
        var w = l_shape(s, u, v, c);
        for (var k = 1u; k < shapes; k++) {
            s += 16u;
            var cw = l_shape(s, u, v, c);
            if (lay[s + 2u] > 0.5) { cw = 1.0 - cw; }
            let mode = u32(lay[s + 1u]);
            if (mode == 0u) {
                w = max(w, cw);
            } else if (mode == 1u) {
                w = w * (1.0 - cw);
            } else {
                w = w * cw;
            }
        }
        at = s + 16u;
        if (invert) { w = 1.0 - w; }
        w = clamp(w * opacity, 0.0, 1.0);
        if (w <= 1e-4) {
            continue;
        }
        var l = c;
        if (abs(exposure) > 1e-4) {
            let mul = exp2(exposure * w);
            l = vec3<f32>(l_encode(l_decode(c.x) * mul), l_encode(l_decode(c.y) * mul), l_encode(l_decode(c.z) * mul));
        }
        if (highlights != 0.0 || shadows != 0.0 || whites != 0.0 || blacks != 0.0) {
            let y = max(0.2126 * l.x + 0.7152 * l.y + 0.0722 * l.z, 0.0);
            var shift = 0.0;
            let hz = min(max(y - 0.35, 0.0) / 0.65, 1.0);
            shift += highlights / 100.0 * hz * hz * (3.0 - 2.0 * hz) * 0.35;
            let fall0 = 1.0 - min(y / 0.60, 1.0);
            shift += shadows / 100.0 * (fall0 * fall0) * (y / (y + 0.03)) * 0.35;
            let wz = min(max(y - 0.60, 0.0) / 0.40, 1.0);
            shift += whites / 100.0 * (wz * wz) * 0.25;
            let bz = min(max(0.30 - y, 0.0) / 0.30, 1.0);
            shift += blacks / 100.0 * (bz * bz) * 0.20;
            l += vec3<f32>(shift * w);
        }
        if (abs(contrast) > 1e-3) {
            let k = 1.0 + (contrast / 100.0) * w;
            l = vec3<f32>(0.41) + (l - vec3<f32>(0.41)) * k;
        }
        if (abs(saturation) > 1e-3) {
            let k = 1.0 + (saturation / 100.0) * w;
            let y = 0.2126 * l.x + 0.7152 * l.y + 0.0722 * l.z;
            l = vec3<f32>(y) + (l - vec3<f32>(y)) * k;
        }
        if (abs(tint.x) > 1e-4 || abs(tint.y) > 1e-4 || abs(tint.z) > 1e-4) {
            l += tint * w * 0.2;
        }
        c = max(l, vec3<f32>(0.0));
    }
    rgb[i * 3u] = c.x;
    rgb[i * 3u + 1u] = c.y;
    rgb[i * 3u + 2u] = c.z;
}

// Lens distortion and perspective (detail::apply_geometry): each output pixel samples
// rgb bilinearly where Geometry::source puts it, black outside the frame; the result
// goes to the planes as interleaved RGB. flags bit 0: perspective, bit 1: lens.
@compute @workgroup_size(16, 16, 1)
fn geometry(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    let cx = p.g[2].z;
    let cy = p.g[2].w;
    var x = f32(id.x);
    var y = f32(id.y);
    var inside = true;
    if ((p.flags & 1u) != 0u) {
        let dx = x - cx;
        let dy = y - cy;
        let m0 = p.g[0];
        let m1 = p.g[1];
        let m2 = p.g[2];
        let w = (m1.z * dx + m1.w * dy) + m2.x;
        if (w <= 1e-6) {
            inside = false;
        } else {
            x = ((m0.x * dx + m0.y * dy) + m0.z) / w + cx;
            y = ((m0.w * dx + m1.x * dy) + m1.y) / w + cy;
        }
    }
    if (inside && (p.flags & 2u) != 0u) {
        let dx = x - cx;
        let dy = y - cy;
        let lens = p.g[3];
        let factor = (1.0 + lens.x * (dx * dx + dy * dy) * lens.y) * lens.z;
        x = cx + dx * factor;
        y = cy + dy * factor;
    }
    var res = vec3<f32>(0.0, 0.0, 0.0);
    if (inside && x >= 0.0 && x < f32(p.w - 1u) && y >= 0.0 && y < f32(p.h - 1u)) {
        let x0 = u32(floor(x));
        let y0 = u32(floor(y));
        let x1 = min(x0 + 1u, p.w - 1u);
        let y1 = min(y0 + 1u, p.h - 1u);
        let fx = x - f32(x0);
        let fy = y - f32(y0);
        let a = y0 * p.w;
        let c = y1 * p.w;
        for (var k = 0u; k < 3u; k++) {
            let top = rgb[(a + x0) * 3u + k] * (1.0 - fx) + rgb[(a + x1) * 3u + k] * fx;
            let bottom = rgb[(c + x0) * 3u + k] * (1.0 - fx) + rgb[(c + x1) * 3u + k] * fx;
            res[k] = top * (1.0 - fy) + bottom * fy;
        }
    }
    pl[i * 3u] = res.x;
    pl[i * 3u + 1u] = res.y;
    pl[i * 3u + 2u] = res.z;
}

// px8 packed to 3 bytes per pixel, one output word per thread; p.src threads per row
@compute @workgroup_size(256, 1, 1)
fn pack3(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x + id.y * p.src;
    let bytes = p.w * p.h * 3u;
    if (k * 4u >= bytes) {
        return;
    }
    var word = 0u;
    for (var j = 0u; j < 4u; j++) {
        let byte = k * 4u + j;
        if (byte < bytes) {
            let v = (px8[byte / 3u] >> (8u * (byte % 3u))) & 255u;
            word = word | (v << (8u * j));
        }
    }
    out8[k] = word;
}

// Histogram workgroups; each walks the frame with this stride. Must match HIST_GROUPS
const HIST_GROUPS: u32 = 256u;

// The CPU's 0.2126 * r, 0.7152 * g and 0.0722 * b for every level, after the counts:
// with only additions left, the GPU can't fuse a multiply into them, so the luma
// level (truncated) comes out exactly as on the CPU
const LUMA_TERMS: u32 = 8450u;

fn luma8(v: u32) -> u32 {
    let r = bitcast<f32>(atomicLoad(&hist[LUMA_TERMS + (v & 255u)]));
    let g = bitcast<f32>(atomicLoad(&hist[LUMA_TERMS + 256u + ((v >> 8u) & 255u)]));
    let b = bitcast<f32>(atomicLoad(&hist[LUMA_TERMS + 512u + ((v >> 16u) & 255u)]));
    return min(u32((r + g) + b), 255u);
}

// Red, green, blue, luma, shadow and highlight clips and the vectorscope: words
// 0..3330 of the layout
var<workgroup> h_main: array<atomic<u32>, 3330>;

@compute @workgroup_size(256, 1, 1)
fn hist_main(@builtin(local_invocation_index) li: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    for (var k = li; k < 3330u; k += 256u) {
        atomicStore(&h_main[k], 0u);
    }
    workgroupBarrier();
    let n = p.w * p.h;
    for (var i = wg.x * 256u + li; i < n; i += 256u * HIST_GROUPS) {
        let v = px8[i];
        let r = v & 255u;
        let g = (v >> 8u) & 255u;
        let b = (v >> 16u) & 255u;
        atomicAdd(&h_main[r], 1u);
        atomicAdd(&h_main[256u + g], 1u);
        atomicAdd(&h_main[512u + b], 1u);
        atomicAdd(&h_main[768u + luma8(v)], 1u);
        if (r >= 254u || g >= 254u || b >= 254u) {
            atomicAdd(&h_main[1025u], 1u);
        } else if (r <= 1u && g <= 1u && b <= 1u) {
            atomicAdd(&h_main[1024u], 1u);
        }
        let rf = f32(r) / 255.0;
        let gf = f32(g) / 255.0;
        let bf = f32(b) / 255.0;
        let cb = -0.1146 * rf - 0.3854 * gf + 0.5000 * bf;
        let cr = 0.5000 * rf - 0.4542 * gf - 0.0458 * bf;
        let u = u32(clamp((cb + 0.5) * 47.0, 0.0, 47.0));
        let vv = u32(clamp(47.0 - (cr + 0.5) * 47.0, 0.0, 47.0));
        atomicAdd(&h_main[1026u + vv * 48u + u], 1u);
    }
    workgroupBarrier();
    for (var k = li; k < 3330u; k += 256u) {
        let c = atomicLoad(&h_main[k]);
        if (c > 0u) {
            atomicAdd(&hist[k], c);
        }
    }
}

// Luma waveform, 64 columns x 32 rows: words 3330..5378
var<workgroup> h_wf: array<atomic<u32>, 2048>;

@compute @workgroup_size(256, 1, 1)
fn hist_wf(@builtin(local_invocation_index) li: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    for (var k = li; k < 2048u; k += 256u) {
        atomicStore(&h_wf[k], 0u);
    }
    workgroupBarrier();
    let n = p.w * p.h;
    for (var i = wg.x * 256u + li; i < n; i += 256u * HIST_GROUPS) {
        let x = i % p.w;
        let wx = min((x * 64u) / p.w, 63u);
        let wy = 31u - (luma8(px8[i]) * 31u) / 255u;
        atomicAdd(&h_wf[wy * 64u + wx], 1u);
    }
    workgroupBarrier();
    for (var k = li; k < 2048u; k += 256u) {
        let c = atomicLoad(&h_wf[k]);
        if (c > 0u) {
            atomicAdd(&hist[3330u + k], c);
        }
    }
}

// RGB parade, 3 x 32 columns x 32 rows: words 5378..8450
var<workgroup> h_par: array<atomic<u32>, 3072>;

@compute @workgroup_size(256, 1, 1)
fn hist_par(@builtin(local_invocation_index) li: u32, @builtin(workgroup_id) wg: vec3<u32>) {
    for (var k = li; k < 3072u; k += 256u) {
        atomicStore(&h_par[k], 0u);
    }
    workgroupBarrier();
    let n = p.w * p.h;
    for (var i = wg.x * 256u + li; i < n; i += 256u * HIST_GROUPS) {
        let v = px8[i];
        let x = i % p.w;
        let col = min((x * 32u) / p.w, 31u);
        for (var c = 0u; c < 3u; c++) {
            let level = (v >> (8u * c)) & 255u;
            let row = 31u - (level * 31u) / 255u;
            atomicAdd(&h_par[c * 1024u + row * 32u + col], 1u);
        }
    }
    workgroupBarrier();
    for (var k = li; k < 3072u; k += 256u) {
        let c = atomicLoad(&h_par[k]);
        if (c > 0u) {
            atomicAdd(&hist[5378u + k], c);
        }
    }
}
