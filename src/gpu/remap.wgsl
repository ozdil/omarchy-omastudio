// The export's lens distortion and perspective remap with vignette and 16-bit
// quantization (pipeline::process_16_to_16_banded's last pass). `src` holds rows
// sy0.. of the frame before the remap as u16 RGB (value / 32767.5), two per word;
// each thread writes one word (two u16 values) of output rows oy0.. .

struct Params {
    w: u32,
    h: u32,
    oy0: u32,
    rows: u32,
    sy0: u32,
    srows: u32,
    flags: u32,
    groups_x: u32,
    vignette: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
    pad3: f32,
    pad4: f32,
    pad5: f32,
    pad6: f32,
    g: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> src: array<u32>;
@group(0) @binding(2) var<storage, read_write> dst: array<u32>;

fn src_value(row: u32, x: u32, c: u32) -> f32 {
    let j = ((row - p.sy0) * p.w + x) * 3u + c;
    let word = src[j / 2u];
    let v = select(word & 0xffffu, word >> 16u, (j & 1u) == 1u);
    return f32(v) / 32767.5;
}

// The 16-bit output value of channel c of frame pixel (x, y)
fn value(x: u32, y: u32, c: u32) -> u32 {
    let cx = p.g[2].z;
    let cy = p.g[2].w;
    var sx = f32(x);
    var sy = f32(y);
    var inside = true;
    if ((p.flags & 1u) != 0u) {
        let dx = sx - cx;
        let dy = sy - cy;
        let m0 = p.g[0];
        let m1 = p.g[1];
        let m2 = p.g[2];
        let w = (m1.z * dx + m1.w * dy) + m2.x;
        if (w <= 1e-6) {
            inside = false;
        } else {
            sx = ((m0.x * dx + m0.y * dy) + m0.z) / w + cx;
            sy = ((m0.w * dx + m1.x * dy) + m1.y) / w + cy;
        }
    }
    if (inside && (p.flags & 2u) != 0u) {
        let dx = sx - cx;
        let dy = sy - cy;
        let lens = p.g[3];
        let factor = (1.0 + lens.x * (dx * dx + dy * dy) * lens.y) * lens.z;
        sx = cx + dx * factor;
        sy = cy + dy * factor;
    }
    var v = 0.0;
    if (inside && sx >= 0.0 && sx < f32(p.w - 1u) && sy >= 0.0 && sy < f32(p.h - 1u)) {
        let x0 = u32(floor(sx));
        let y0 = u32(floor(sy));
        let x1 = min(x0 + 1u, p.w - 1u);
        let y1 = min(y0 + 1u, p.h - 1u);
        // Rows outside the uploaded span clamp to it (the span is sized not to need it)
        let r0 = clamp(y0, p.sy0, p.sy0 + p.srows - 1u);
        let r1 = clamp(y1, p.sy0, p.sy0 + p.srows - 1u);
        let fx = sx - f32(x0);
        let fy = sy - f32(y0);
        let top = src_value(r0, x0, c) * (1.0 - fx) + src_value(r0, x1, c) * fx;
        let bottom = src_value(r1, x0, c) * (1.0 - fx) + src_value(r1, x1, c) * fx;
        v = top * (1.0 - fy) + bottom * fy;
    }
    if ((p.flags & 4u) != 0u) {
        let hx = f32(p.w) / 2.0;
        let hy = f32(p.h) / 2.0;
        let max_dist = sqrt(hx * hx + hy * hy);
        let dx = f32(x) - hx;
        let dy = f32(y) - hy;
        let dist = sqrt(dx * dx + dy * dy) / max_dist;
        let falloff = clamp((dist - 0.4) / 0.6, 0.0, 1.0);
        let smoothed = falloff * falloff * (3.0 - 2.0 * falloff);
        v = v * max(1.0 + (p.vignette / 100.0) * smoothed, 0.0);
    }
    return u32(clamp(v, 0.0, 1.0) * 65535.0 + 0.5);
}

@compute @workgroup_size(256, 1, 1)
fn remap(@builtin(global_invocation_id) id: vec3<u32>) {
    let k = id.x + id.y * p.groups_x * 256u;
    let count = p.w * p.rows * 3u;
    if (k * 2u >= count) {
        return;
    }
    var word = 0u;
    for (var j = 0u; j < 2u; j++) {
        let n = k * 2u + j;
        if (n < count) {
            let px = n / 3u;
            let v = value(px % p.w, p.oy0 + px / p.w, n % 3u);
            word = word | (v << (16u * j));
        }
    }
    dst[k] = word;
}
