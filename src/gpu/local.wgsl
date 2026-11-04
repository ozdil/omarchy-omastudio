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
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read_write> rgb: array<f32>;
@group(0) @binding(2) var<storage, read_write> pl: array<f32>;
// The display tail: final 8-bit pixels (R | G << 8 | B << 16), the same packed to
// 3 bytes per pixel, and the histogram and scope counts (layout in histogram.rs)
@group(0) @binding(3) var<storage, read_write> px8: array<u32>;
@group(0) @binding(4) var<storage, read_write> out8: array<u32>;
@group(0) @binding(5) var<storage, read_write> hist: array<atomic<u32>>;

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
// process_buffer_16_to_8_ex does on the CPU
@compute @workgroup_size(16, 16, 1)
fn finish(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = pixel(id);
    if (i >= p.w * p.h) {
        return;
    }
    var r = rgb[i * 3u];
    var g = rgb[i * 3u + 1u];
    var b = rgb[i * 3u + 2u];
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
