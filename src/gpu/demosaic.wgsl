// GPU version of raw::demosaic (edge-directed colour-difference demosaic with
// homogeneity selection and a false-colour median). Each entry point is one stage
// of the CPU tile code, run over a band of image rows; see demosaic.rs for the
// algorithm. Values and operation order follow the CPU code so the two agree.

struct Params {
    width: u32,
    height: u32,
    // First image row held in `mosaic`
    mosaic_y0: u32,
    // Extended rows [ext_y0, ext_y1): green, candidates and homogeneity
    ext_y0: u32,
    ext_y1: u32,
    // Output rows [out_y0, out_y1); selection covers one more row on each side
    out_y0: u32,
    out_y1: u32,
    sel_y0: u32,
    sel_y1: u32,
    period: u32,
    _pad0: u32,
    _pad1: u32,
    // Camera RGB -> output space, rows in xyz
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
}

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read> mosaic: array<f32>;
@group(0) @binding(2) var<storage, read> phases: array<u32>;
@group(0) @binding(3) var<storage, read_write> green: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> cand: array<f32>;
@group(0) @binding(5) var<storage, read_write> homo: array<u32>;
@group(0) @binding(6) var<storage, read_write> sel: array<f32>;
@group(0) @binding(7) var<storage, read_write> outbuf: array<u32>;

const REACH: i32 = 3;
const EPS: f32 = 1e-4;
const STRIDE: u32 = 159u;
const WIN: u32 = 24u;
const BIG: f32 = 3.402823e38;

fn dir(d: i32) -> vec2<i32> {
    // (dy, dx)
    switch d {
        case 0: { return vec2<i32>(0, 1); }
        case 1: { return vec2<i32>(1, 0); }
        case 2: { return vec2<i32>(1, 1); }
        default: { return vec2<i32>(1, -1); }
    }
}

fn inside(y: i32, x: i32) -> bool {
    return y >= 0 && x >= 0 && y < i32(P.height) && x < i32(P.width);
}

fn raw(y: i32, x: i32) -> f32 {
    return mosaic[u32(y - i32(P.mosaic_y0)) * P.width + u32(x)];
}

fn phase_base(y: i32, x: i32) -> u32 {
    let p = (u32(y) % P.period) * P.period + u32(x) % P.period;
    return p * STRIDE;
}

fn ext_index(y: i32, x: i32) -> u32 {
    return u32(y - i32(P.ext_y0)) * P.width + u32(x);
}

@compute @workgroup_size(16, 16)
fn green_pass(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = i32(id.x);
    let y = i32(P.ext_y0 + id.y);
    if (id.x >= P.width || u32(y) >= P.ext_y1) { return; }
    let b = phase_base(y, x);
    let color = phases[b];
    let centre = raw(y, x);
    if (color == 1u) {
        green[ext_index(y, x)] = vec4<f32>(centre);
        return;
    }
    var est = array<f32, 4>(0.0, 0.0, 0.0, 0.0);
    var defined = array<bool, 4>(false, false, false, false);
    var sum = 0.0;
    var weight = 0.0;
    var lo = BIG;
    var hi = -BIG;
    for (var d = 0; d < 4; d++) {
        let plus = i32(phases[b + 1u + u32(d) * 3u]);
        if (plus == 0) { continue; }
        let minus = i32(phases[b + 2u + u32(d) * 3u]);
        let same = i32(phases[b + 3u + u32(d) * 3u]);
        let dv = dir(d);
        let yp = y + dv.x * plus;
        let xp = x + dv.y * plus;
        let ym = y - dv.x * minus;
        let xm = x - dv.y * minus;
        if (!inside(yp, xp) || !inside(ym, xm)) { continue; }
        let gp = raw(yp, xp);
        let gm = raw(ym, xm);
        let kp = f32(plus);
        let km = f32(minus);
        var e = (gp * km + gm * kp) / (kp + km);
        var gradient = abs(gp - gm) / (kp + km);
        if (same > 0) {
            let ya = y + dv.x * same;
            let xa = x + dv.y * same;
            let yb = y - dv.x * same;
            let xb = x - dv.y * same;
            if (inside(ya, xa) && inside(yb, xb)) {
                let lap = 2.0 * centre - raw(ya, xa) - raw(yb, xb);
                let s2 = f32(same * same);
                e += lap * kp * km / s2;
                gradient += abs(lap) / s2;
            }
        }
        e = clamp(e, min(gp, gm), max(gp, gm));
        lo = min(lo, min(gp, gm));
        hi = max(hi, max(gp, gm));
        est[d] = e;
        defined[d] = true;
        let wgt = 1.0 / ((EPS + gradient) * (EPS + gradient) * (kp + km));
        sum += wgt * e;
        weight += wgt;
    }
    var blend = centre;
    if (weight > 0.0) {
        blend = clamp(sum / weight, lo, hi);
    } else {
        var s = 0.0;
        var k = 0.0;
        for (var dy = -2; dy <= 2; dy++) {
            for (var dx = -2; dx <= 2; dx++) {
                let yy = y + dy;
                let xx = x + dx;
                if (inside(yy, xx) && phases[phase_base(yy, xx)] == 1u) {
                    s += raw(yy, xx);
                    k += 1.0;
                }
            }
        }
        if (k > 0.0) { blend = s / k; }
    }
    var g = vec4<f32>(blend);
    for (var d = 0; d < 4; d++) {
        if (defined[d]) { g[d] = est[d]; }
    }
    green[ext_index(y, x)] = g;
}

@compute @workgroup_size(16, 16)
fn cand_pass(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = i32(id.x);
    let y = i32(P.ext_y0 + id.y);
    if (id.x >= P.width || u32(y) >= P.ext_y1) { return; }
    let b = phase_base(y, x);
    let color = phases[b];
    let i = ext_index(y, x);
    let local = raw(y, x);
    for (var d = 0; d < 4; d++) {
        let g0 = green[i][d];
        var px = vec3<f32>(0.0, g0, 0.0);
        for (var k = 0u; k < 2u; k++) {
            let c = k * 2u;
            if (color == c) {
                px[c] = local;
                continue;
            }
            let n = phases[b + 13u + k];
            var sum = 0.0;
            var weight = 0.0;
            for (var e = 0u; e < n; e++) {
                let o = b + 15u + k * WIN * 3u + e * 3u;
                let yy = y + bitcast<i32>(phases[o]);
                let xx = x + bitcast<i32>(phases[o + 1u]);
                let wd = bitcast<f32>(phases[o + 2u]);
                if (yy < i32(P.ext_y0) || yy >= i32(P.ext_y1) || xx < 0 || xx >= i32(P.width)) { continue; }
                let gj = green[ext_index(yy, xx)][d];
                let edge = EPS + abs(gj - g0);
                let wgt = wd / (edge * edge);
                sum += wgt * (raw(yy, xx) - gj);
                weight += wgt;
            }
            var diff = 0.0;
            if (weight > 0.0) { diff = sum / weight; }
            px[c] = max(g0 + diff, 0.0);
        }
        let o = (i * 4u + u32(d)) * 3u;
        cand[o] = px.x;
        cand[o + 1u] = px.y;
        cand[o + 2u] = px.z;
    }
}

fn ycc(i: u32, d: u32) -> vec3<f32> {
    let o = (i * 4u + d) * 3u;
    let r = sqrt(max(cand[o], 0.0));
    let g = sqrt(max(cand[o + 1u], 0.0));
    let b = sqrt(max(cand[o + 2u], 0.0));
    let yv = 0.25 * r + 0.5 * g + 0.25 * b;
    return vec3<f32>(yv, r - g, b - g);
}

fn chroma_dist(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let u = a.y - b.y;
    let v = a.z - b.z;
    return u * u + v * v;
}

@compute @workgroup_size(16, 16)
fn homo_pass(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = i32(id.x);
    let y = i32(P.ext_y0 + id.y);
    if (id.x >= P.width || u32(y) >= P.ext_y1) { return; }
    let i = ext_index(y, x);
    // Like the CPU tile, the outermost rows and columns of the region score zero
    if (u32(y) == P.ext_y0 || u32(y) + 1u == P.ext_y1 || x == 0 || x + 1 == i32(P.width)) {
        homo[i] = 0u;
        return;
    }
    var el = BIG;
    var ec = BIG;
    for (var d = 0u; d < 4u; d++) {
        let dv = dir(i32(d));
        let a = ext_index(y + dv.x, x + dv.y);
        let bb = ext_index(y - dv.x, x - dv.y);
        let c = ycc(i, d);
        let pa = ycc(a, d);
        let pb = ycc(bb, d);
        let dl = max(abs(c.x - pa.x), abs(c.x - pb.x));
        let dc = max(chroma_dist(c, pa), chroma_dist(c, pb));
        el = min(el, dl);
        ec = min(ec, dc);
    }
    var packed = 0u;
    for (var d = 0u; d < 4u; d++) {
        let c = ycc(i, d);
        var count = 0u;
        for (var dy = -1; dy <= 1; dy++) {
            for (var dx = -1; dx <= 1; dx++) {
                let q = ycc(ext_index(y + dy, x + dx), d);
                if (abs(c.x - q.x) <= el && chroma_dist(c, q) <= ec) { count += 1u; }
            }
        }
        packed |= count << (8u * d);
    }
    homo[i] = packed;
}

@compute @workgroup_size(16, 16)
fn select_pass(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = i32(id.x);
    let y = i32(P.sel_y0 + id.y);
    if (id.x >= P.width || u32(y) >= P.sel_y1) { return; }
    var score = vec4<u32>(0u);
    let ya = max(y - 2, i32(P.ext_y0));
    let yb = min(y + 2, i32(P.ext_y1) - 1);
    let xa = max(x - 2, 0);
    let xb = min(x + 2, i32(P.width) - 1);
    for (var yy = ya; yy <= yb; yy++) {
        for (var xx = xa; xx <= xb; xx++) {
            let h = homo[ext_index(yy, xx)];
            score += vec4<u32>(h & 255u, (h >> 8u) & 255u, (h >> 16u) & 255u, h >> 24u);
        }
    }
    let best = max(max(score.x, score.y), max(score.z, score.w));
    let keep = best - best / 8u;
    var acc = vec3<f32>(0.0);
    var k = 0.0;
    let i = ext_index(y, x);
    for (var d = 0u; d < 4u; d++) {
        if (score[d] >= keep) {
            let o = (i * 4u + d) * 3u;
            acc += vec3<f32>(cand[o], cand[o + 1u], cand[o + 2u]);
            k += 1.0;
        }
    }
    let s = (u32(y) - P.sel_y0) * P.width + u32(x);
    sel[s * 3u] = acc.x / k;
    sel[s * 3u + 1u] = acc.y / k;
    sel[s * 3u + 2u] = acc.z / k;
}

fn sel_at(y: i32, x: i32) -> vec3<f32> {
    let s = (u32(y) - P.sel_y0) * P.width + u32(x);
    return vec3<f32>(sel[s * 3u], sel[s * 3u + 1u], sel[s * 3u + 2u]);
}

// Median of nine by the same exchange network as the CPU code
fn median9(v: array<f32, 9>) -> f32 {
    var p = v;
    var pairs = array<vec2<u32>, 19>(
        vec2(1u, 2u), vec2(4u, 5u), vec2(7u, 8u), vec2(0u, 1u), vec2(3u, 4u), vec2(6u, 7u),
        vec2(1u, 2u), vec2(4u, 5u), vec2(7u, 8u), vec2(0u, 3u), vec2(5u, 8u), vec2(4u, 7u),
        vec2(3u, 6u), vec2(1u, 4u), vec2(2u, 5u), vec2(4u, 7u), vec2(4u, 2u), vec2(6u, 4u),
        vec2(4u, 2u));
    for (var k = 0u; k < 19u; k++) {
        let a = pairs[k].x;
        let b = pairs[k].y;
        if (p[a] > p[b]) {
            let t = p[a];
            p[a] = p[b];
            p[b] = t;
        }
    }
    return p[4];
}

@compute @workgroup_size(16, 16)
fn median_pass(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = i32(id.x);
    let y = i32(P.out_y0 + id.y);
    if (id.x >= P.width || u32(y) >= P.out_y1) { return; }
    var px = sel_at(y, x);
    if (P.width >= 3u && P.height >= 3u && y > 0 && y + 1 < i32(P.height) && x > 0 && x + 1 < i32(P.width)) {
        let g = px.y;
        for (var c = 0u; c < 3u; c += 2u) {
            var v: array<f32, 9>;
            for (var k = 0; k < 9; k++) {
                let q = sel_at(y + k / 3 - 1, x + k % 3 - 1);
                v[k] = q[c] - q.y;
            }
            px[c] = max(g + median9(v), 0.0);
        }
    }
    let o = ((u32(y) - P.out_y0) * P.width + u32(x)) * 3u;
    var m = array<vec3<f32>, 3>(P.m0.xyz, P.m1.xyz, P.m2.xyz);
    for (var r = 0u; r < 3u; r++) {
        let v = m[r].x * px.x + m[r].y * px.y + m[r].z * px.z;
        outbuf[o + r] = u32(round(clamp(v, 0.0, 1.0) * 65535.0));
    }
}
