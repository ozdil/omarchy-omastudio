// The creative per-pixel operations of the preview and export pipeline, on the GPU.
// Mirrors, function by function, crate::pipeline::PointOps::apply after the
// scene-linear stage (white balance, exposure, display curve): tone, point curves,
// RGB mixer, skin tone, colour wheels, HSL mixer, look LUT, film simulation,
// Color Chrome and ACES gamut compression. Keep both sides in step: the gpu_matches_cpu
// test in point_ops.rs compares them on every module.
//
// The P_* parameter indices are generated from point_ops.rs and prepended to this
// source, so the two cannot drift apart.

@group(0) @binding(0) var<storage, read> params: array<u32>;
// 16-bit input samples, two per word, little-endian
@group(0) @binding(1) var<storage, read> src: array<u32>;
// Curve tables (4 x CURVE_SIZE) followed by the look and film LUTs
@group(0) @binding(2) var<storage, read> tables: array<f32>;
// Interleaved RGB f32 output for the band
@group(0) @binding(3) var<storage, read_write> dst: array<f32>;

const PI: f32 = 3.14159265358979;
const CURVE_SIZE: u32 = 1024u;

fn pu(i: u32) -> u32 { return params[i]; }
fn pf(i: u32) -> f32 { return bitcast<f32>(params[i]); }
fn on(i: u32) -> bool { return params[i] != 0u; }

fn luma709(c: vec3<f32>) -> f32 { return 0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z; }

fn sample16(i: u32) -> f32 {
    let word = src[i >> 1u];
    let v = select(word & 0xffffu, word >> 16u, (i & 1u) == 1u);
    return f32(v);
}

// --- Transfer curves (pipeline::transfer::Transfer) ---------------------------------

fn encode_with(lin: f32, power: f32, slope: f32, toe_linear: f32, offset: f32) -> f32 {
    if (!(lin > 0.0)) { return 0.0; }
    if (lin < toe_linear) { return lin * slope; }
    return pow(lin, power) * (1.0 + offset) - offset;
}

fn bt709_encode(lin: f32) -> f32 {
    return encode_with(lin, pf(P_BT_POWER), pf(P_BT_SLOPE), pf(P_BT_TOE_LINEAR), pf(P_BT_OFFSET));
}

fn bt709_decode(enc: f32) -> f32 {
    if (!(enc > 0.0)) { return 0.0; }
    if (enc < pf(P_BT_TOE_ENCODED)) { return enc / pf(P_BT_SLOPE); }
    let o = pf(P_BT_OFFSET);
    return pow((enc + o) / (1.0 + o), 1.0 / pf(P_BT_POWER));
}

// --- Tone (pipeline::tone) ----------------------------------------------------------

fn reconstruct_clipped_highlights(c: vec3<f32>) -> vec3<f32> {
    let t = 0.94;
    let rc = c.x > t;
    let gc = c.y > t;
    let bc = c.z > t;
    if (!rc && !gc && !bc) { return c; }
    let max_val = max(max(c.x, c.y), c.z);
    let excess = clamp((max_val - t) / (1.25 - t), 0.0, 1.0);
    let desat = excess * excess * (3.0 - 2.0 * excess) * 0.65;
    let e = max_val;
    let ro = select(c.x + (e - c.x) * desat * 0.5, e, rc);
    let go = select(c.y + (e - c.y) * desat * 0.5, e, gc);
    let bo = select(c.z + (e - c.z) * desat * 0.5, e, bc);
    let l = 0.2126 * ro + 0.7152 * go + 0.0722 * bo;
    return vec3<f32>(ro * (1.0 - desat) + l * desat, go * (1.0 - desat) + l * desat, bo * (1.0 - desat) + l * desat);
}

fn agx_curve(v: f32) -> f32 {
    if (!(v > 0.0)) { return 0.0; }
    let lv = (clamp(log2(v), -10.0, 4.0) + 10.0) / 14.0;
    return clamp(lv * lv * (3.0 - 2.0 * lv), 0.0, 1.0);
}

fn aces_tonemap(v: f32) -> f32 {
    let x = max(v, 0.0) * 0.6;
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), 0.0, 1.0);
}

fn aces_tonemap_encoded(v: f32) -> f32 {
    let lin = pow(max(v, 0.0), 2.2);
    return pow(aces_tonemap(lin), 1.0 / 2.2);
}

fn tone(cin: vec3<f32>) -> vec3<f32> {
    var c = cin;
    if (on(P_HIGHLIGHT_RECONSTRUCT)) { c = reconstruct_clipped_highlights(c); }
    let luma = luma709(c);
    if (luma <= 0.0) { return vec3<f32>(0.0); }
    var la = luma;

    let highlights = pf(P_HIGHLIGHTS);
    if (highlights != 0.0) {
        let h = min(max(luma - 0.35, 0.0) / 0.65, 1.0);
        la += (highlights / 100.0) * (h * h * (3.0 - 2.0 * h)) * 0.35;
    }
    let shadows = pf(P_SHADOWS);
    if (shadows != 0.0) {
        let s = clamp(luma / 0.60, 0.0, 1.0);
        let falloff = (1.0 - s) * (1.0 - s);
        let toe = clamp(luma / (luma + 0.03), 0.0, 1.0);
        la += (shadows / 100.0) * falloff * toe * 0.35;
    }
    let whites = pf(P_WHITES);
    if (whites != 0.0) {
        let w = min(max(luma - 0.60, 0.0) / 0.40, 1.0);
        la += (whites / 100.0) * w * w * 0.25;
    }
    let blacks = pf(P_BLACKS);
    if (blacks != 0.0) {
        let b = min(max(0.30 - luma, 0.0) / 0.30, 1.0);
        la += (blacks / 100.0) * b * b * 0.20;
    }
    let contrast = pf(P_CONTRAST);
    if (contrast != 0.0) {
        let k = contrast / 100.0;
        let pivot = clamp(pf(P_CONTRAST_PIVOT), 0.05, 0.95);
        let d = la - pivot;
        la = pivot + d * (1.0 + k * 0.5) + (d * d * d) * k * 0.3;
    }
    let chl = pf(P_CURVE_HIGHLIGHTS);
    let cli = pf(P_CURVE_LIGHTS);
    let cda = pf(P_CURVE_DARKS);
    let csh = pf(P_CURVE_SHADOWS);
    if (chl != 0.0 || cli != 0.0 || cda != 0.0 || csh != 0.0) {
        let ch = (chl / 100.0) * max(la - 0.75, 0.0) / 0.25;
        let cl = (cli / 100.0) * (max(la - 0.5, 0.0) * max(0.75 - la, 0.0) * 4.0);
        let cd = (cda / 100.0) * (max(la - 0.25, 0.0) * max(0.5 - la, 0.0) * 4.0);
        let cs = (csh / 100.0) * max(0.25 - la, 0.0) / 0.25;
        la += (ch + cl + cd + cs) * 0.25;
    }
    if (on(P_FILMIC_AGX)) {
        la = agx_curve(max(la, 0.0));
    } else if (on(P_ACES_TONEMAP)) {
        la = aces_tonemap_encoded(la);
    } else {
        la = clamp(la, 0.0, 1.5);
    }

    let ratio = select(1.0, la / luma, luma > 1e-5);
    var chroma_scale: f32;
    if (ratio > 1.0) {
        chroma_scale = min(pow(ratio, 0.20), 1.35);
    } else {
        chroma_scale = pow(ratio, 0.40);
    }
    var o = vec3<f32>(la) + (c - vec3<f32>(luma)) * chroma_scale;

    let max_comp = max(max(o.x, o.y), o.z);
    if (max_comp > 0.88 && la > 0.65) {
        let desat = clamp((max_comp - 0.88) / 0.30, 0.0, 1.0) * 0.35;
        o = o * (1.0 - desat) + vec3<f32>(la * desat);
    }

    let max_c = max(max(o.x, o.y), o.z);
    let min_c = min(min(o.x, o.y), o.z);
    var cur_sat = 0.0;
    if (max_c > 0.0001) { cur_sat = clamp((max_c - max(min_c, 0.0)) / max_c, 0.0, 1.0); }
    var sat_delta = pf(P_SATURATION) / 100.0;
    let vibrance = pf(P_VIBRANCE);
    if (vibrance != 0.0) { sat_delta += (1.0 - cur_sat) * (vibrance / 100.0); }
    let boost = pf(P_COLOR_BOOST);
    if (boost != 0.0) {
        let base = clamp(1.0 - cur_sat, 0.0, 1.0);
        sat_delta += (boost / 100.0) * pow(base, 1.4);
    }
    if (sat_delta != 0.0) {
        let cl = luma709(o);
        let m = max(1.0 + sat_delta, 0.0);
        o = vec3<f32>(cl) + (o - vec3<f32>(cl)) * m;
    }
    return o;
}

// --- Point curves (pipeline::curves) ------------------------------------------------

fn curve(which: u32, x: f32) -> f32 {
    let base = which * CURVE_SIZE;
    let last = CURVE_SIZE - 1u;
    if (x <= 0.0) { return tables[base] + x; }
    if (x >= 1.0) { return tables[base + last] + (x - 1.0); }
    let pos = x * f32(last);
    let i = min(u32(pos), last - 1u);
    let t = pos - f32(i);
    let a = tables[base + i];
    return a + (tables[base + i + 1u] - a) * t;
}

fn curves(cin: vec3<f32>) -> vec3<f32> {
    var c = cin;
    if (on(P_CURVE_MASTER)) { c = vec3<f32>(curve(0u, c.x), curve(0u, c.y), curve(0u, c.z)); }
    if (on(P_CURVE_RED)) { c.x = curve(1u, c.x); }
    if (on(P_CURVE_GREEN)) { c.y = curve(2u, c.y); }
    if (on(P_CURVE_BLUE)) { c.z = curve(3u, c.z); }
    return c;
}

// --- RGB mixer (pipeline::rgb_mixer) ------------------------------------------------

fn rgb_mixer(c: vec3<f32>) -> vec3<f32> {
    if (!on(P_MIXER_ENABLED)) { return c; }
    let rr = pf(P_MIXER + 0u); let gr = pf(P_MIXER + 1u); let br = pf(P_MIXER + 2u);
    if (on(P_MIXER_MONO)) {
        let total = abs(rr + gr + br);
        let norm = select(1.0, total, total > 1e-4);
        let m = max((c.x * rr + c.y * gr + c.z * br) / norm, 0.0);
        return vec3<f32>(m);
    }
    let rg = pf(P_MIXER + 3u); let gg = pf(P_MIXER + 4u); let bg = pf(P_MIXER + 5u);
    let rb = pf(P_MIXER + 6u); let gb = pf(P_MIXER + 7u); let bb = pf(P_MIXER + 8u);
    return vec3<f32>(
        max(c.x * rr + c.y * gr + c.z * br, 0.0),
        max(c.x * rg + c.y * gg + c.z * bg, 0.0),
        max(c.x * rb + c.y * gb + c.z * bb, 0.0),
    );
}

// --- Oklab (pipeline::color_grading) -------------------------------------------------

fn cbrt_pos(v: f32) -> f32 { return select(0.0, pow(v, 1.0 / 3.0), v > 0.0); }

fn linear_to_oklab(c: vec3<f32>) -> vec3<f32> {
    let l = 0.4122214708 * c.x + 0.5363325363 * c.y + 0.0514459929 * c.z;
    let m = 0.2119034982 * c.x + 0.6806995451 * c.y + 0.1073969566 * c.z;
    let s = 0.0883024619 * c.x + 0.2817188376 * c.y + 0.6299787005 * c.z;
    let l_ = cbrt_pos(l);
    let m_ = cbrt_pos(m);
    let s_ = cbrt_pos(s);
    return vec3<f32>(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    );
}

fn oklab_to_linear(lab: vec3<f32>) -> vec3<f32> {
    let l_ = lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z;
    let m_ = lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z;
    let s_ = lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    return max(vec3<f32>(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ), vec3<f32>(0.0));
}

fn encoded_to_oklab(c: vec3<f32>) -> vec3<f32> {
    return linear_to_oklab(vec3<f32>(bt709_decode(c.x), bt709_decode(c.y), bt709_decode(c.z)));
}

fn oklab_to_encoded(lab: vec3<f32>) -> vec3<f32> {
    let c = oklab_to_linear(lab);
    return vec3<f32>(bt709_encode(c.x), bt709_encode(c.y), bt709_encode(c.z));
}

fn hue_degrees(a: f32, b: f32) -> f32 {
    var h = atan2(b, a) * (180.0 / PI);
    if (h < 0.0) { h += 360.0; }
    return h;
}

// --- Skin tone (pipeline::skin_tone) ------------------------------------------------

fn skin_weight(hue: f32, chroma: f32, luma: f32, aim: f32, tol: f32) -> f32 {
    if (!(luma >= 0.08 && luma <= 0.98)) { return 0.0; }
    var lg = 1.0;
    if (luma < 0.20) { lg = (luma - 0.08) / 0.12; } else if (luma > 0.90) { lg = (0.98 - luma) / 0.08; }
    if (!(chroma >= 0.025 && chroma <= 0.28)) { return 0.0; }
    var cg = 1.0;
    if (chroma < 0.045) { cg = (chroma - 0.025) / 0.020; } else if (chroma > 0.22) { cg = (0.28 - chroma) / 0.060; }
    var diff = abs(hue - aim);
    if (diff > 180.0) { diff = 360.0 - diff; }
    if (diff >= tol) { return 0.0; }
    let hg = 0.5 * (1.0 + cos(PI * diff / tol));
    return clamp(lg * cg * hg, 0.0, 1.0);
}

fn skin_tone(c: vec3<f32>) -> vec3<f32> {
    if (!on(P_SKIN_ENABLED)) { return c; }
    let aim = pf(P_SKIN_TARGET);
    let tol = pf(P_SKIN_TOL);
    let lab = encoded_to_oklab(c);
    let chroma = sqrt(lab.y * lab.y + lab.z * lab.z);
    let hue = hue_degrees(lab.y, lab.z);
    let w = skin_weight(hue, chroma, lab.x, aim, tol);
    if (w <= 0.001) { return c; }
    let u_hue = (clamp(pf(P_SKIN_U_HUE), 0.0, 100.0) / 100.0) * w;
    let u_sat = (clamp(pf(P_SKIN_U_SAT), 0.0, 100.0) / 100.0) * w;
    let d_hue = (clamp(pf(P_SKIN_A_HUE), -100.0, 100.0) / 100.0) * 15.0;
    let d_sat = (clamp(pf(P_SKIN_A_SAT), -100.0, 100.0) / 100.0) * 0.40;
    var diff = aim - hue;
    if (diff > 180.0) { diff -= 360.0; } else if (diff < -180.0) { diff += 360.0; }
    var nh = hue + diff * u_hue + d_hue * w;
    if (nh < 0.0) { nh += 360.0; } else if (nh >= 360.0) { nh -= 360.0; }
    let nc = max(chroma + (0.115 - chroma) * u_sat + chroma * d_sat * w, 0.0);
    let rad = nh * (PI / 180.0);
    return oklab_to_encoded(vec3<f32>(lab.x, nc * cos(rad), nc * sin(rad)));
}

// --- Colour wheels and HSL mixer (pipeline::color_grading) ---------------------------

fn color_wheels(c: vec3<f32>) -> vec3<f32> {
    if (!on(P_WHEELS_ACTIVE)) { return c; }
    let y = clamp(luma709(c), 0.0, 1.0);
    let wl = max(1.0 - y, 0.0) * max(1.0 - y, 0.0);
    let wg = y * y;
    let wm = max(1.0 - wl - wg, 0.0);
    var out = c;
    for (var i = 0u; i < 3u; i++) {
        let d = wl * (pf(P_LIFT + i) * 0.35 + pf(P_LIFT + 3u) * 0.35)
              + wm * (pf(P_GAMMA + i) * 0.35 + pf(P_GAMMA + 3u) * 0.35)
              + wg * (pf(P_GAIN + i) * 0.35 + pf(P_GAIN + 3u) * 0.35)
              + (pf(P_OFFSET + i) * 0.25 + pf(P_OFFSET + 3u) * 0.25);
        out[i] = max(c[i] + d, 0.0);
    }
    return out;
}

const HSL_CENTERS = array<f32, 8>(29.0, 55.0, 105.0, 142.0, 195.0, 264.0, 305.0, 345.0);
const HSL_WIDTHS = array<f32, 8>(42.0, 46.0, 48.0, 55.0, 62.0, 65.0, 48.0, 45.0);

fn hsl_mixer(c: vec3<f32>) -> vec3<f32> {
    if (!on(P_HSL_ACTIVE)) { return c; }
    let lab = encoded_to_oklab(c);
    let chroma = sqrt(lab.y * lab.y + lab.z * lab.z);
    if (chroma < 0.006 || lab.x < 0.001) { return c; }
    let hue = hue_degrees(lab.y, lab.z);
    var dh = 0.0;
    var ds = 0.0;
    var dl = 0.0;
    for (var i = 0u; i < 8u; i++) {
        let bw = HSL_WIDTHS[i];
        var diff = abs(hue - HSL_CENTERS[i]);
        if (diff > 180.0) { diff = 360.0 - diff; }
        if (diff < bw) {
            let w = 0.5 * (1.0 + cos(PI * diff / bw));
            let h = pf(P_HSL_HUE + i);
            let s = pf(P_HSL_SAT + i);
            let l = pf(P_HSL_LUM + i);
            if (h != 0.0) { dh += (h / 100.0) * 35.0 * w; }
            if (s != 0.0) { ds += (s / 100.0) * w; }
            if (l != 0.0) { dl += (l / 100.0) * 0.22 * w; }
        }
    }
    if (dh == 0.0 && ds == 0.0 && dl == 0.0) { return c; }
    var nh = (hue + dh) % 360.0;
    if (nh < 0.0) { nh += 360.0; }
    let rad = nh * (PI / 180.0);
    var nc: f32;
    if (ds >= 0.0) {
        let boosted = chroma * (1.0 + ds * 1.35);
        nc = boosted / (1.0 + 0.12 * ds * min(chroma / 0.35, 1.0));
    } else {
        nc = max(chroma * (1.0 + ds), 0.0);
    }
    let cw = clamp(chroma / 0.12, 0.2, 1.0);
    let nl = clamp(lab.x + dl * cw, 0.0, 1.5);
    return oklab_to_encoded(vec3<f32>(nl, nc * cos(rad), nc * sin(rad)));
}

// --- 3D LUTs (pipeline::lut::Lut3D::sample) ------------------------------------------

// A LUT's parameters start at `p`: size, table offset (in floats), domain min[3], max[3]
fn lut_entry(off: u32, i: u32) -> vec3<f32> {
    let k = off + i * 3u;
    return vec3<f32>(tables[k], tables[k + 1u], tables[k + 2u]);
}

fn lut_sample(p: u32, c: vec3<f32>) -> vec3<f32> {
    let n = pu(p);
    let off = pu(p + 1u);
    let dmin = vec3<f32>(pf(p + 2u), pf(p + 3u), pf(p + 4u));
    let dmax = vec3<f32>(pf(p + 5u), pf(p + 6u), pf(p + 7u));
    let norm = clamp((c - dmin) / (dmax - dmin), vec3<f32>(0.0), vec3<f32>(1.0));
    let pos = norm * f32(n - 1u);
    let i0 = min(vec3<u32>(floor(pos)), vec3<u32>(n - 1u));
    let i1 = min(i0 + vec3<u32>(1u), vec3<u32>(n - 1u));
    let f = pos - vec3<f32>(i0);
    let nn = n * n;
    let c000 = lut_entry(off, i0.z * nn + i0.y * n + i0.x);
    let c100 = lut_entry(off, i0.z * nn + i0.y * n + i1.x);
    let c010 = lut_entry(off, i0.z * nn + i1.y * n + i0.x);
    let c110 = lut_entry(off, i0.z * nn + i1.y * n + i1.x);
    let c001 = lut_entry(off, i1.z * nn + i0.y * n + i0.x);
    let c101 = lut_entry(off, i1.z * nn + i0.y * n + i1.x);
    let c011 = lut_entry(off, i1.z * nn + i1.y * n + i0.x);
    let c111 = lut_entry(off, i1.z * nn + i1.y * n + i1.x);
    let c00 = c000 * (1.0 - f.x) + c100 * f.x;
    let c10 = c010 * (1.0 - f.x) + c110 * f.x;
    let c01 = c001 * (1.0 - f.x) + c101 * f.x;
    let c11 = c011 * (1.0 - f.x) + c111 * f.x;
    let c0 = c00 * (1.0 - f.y) + c10 * f.y;
    let c1 = c01 * (1.0 - f.y) + c11 * f.y;
    return c0 * (1.0 - f.z) + c1 * f.z;
}

fn lut_apply(p: u32, c: vec3<f32>, intensity: f32) -> vec3<f32> {
    if (intensity <= 0.001) { return c; }
    let l = lut_sample(p, c);
    if (intensity >= 0.999) { return l; }
    return c * (1.0 - intensity) + l * intensity;
}

// --- Film simulations (pipeline::film_sim) -------------------------------------------

fn s_curve(y: f32, pivot: f32, k: f32, amp: f32) -> f32 {
    let lo = pivot + tanh((0.0 - pivot) * k) * amp;
    let hi = pivot + tanh((1.0 - pivot) * k) * amp;
    let v = pivot + tanh((clamp(y, 0.0, 1.0) - pivot) * k) * amp;
    return clamp((v - lo) / (hi - lo), 0.0, 1.0);
}

fn mono_curve(y: f32) -> f32 {
    let d = clamp(y, 0.0, 1.0) - 0.46;
    return s_curve(d + 0.46, 0.46, 1.10, 0.52);
}

fn acros_curve(y: f32) -> f32 {
    let d = clamp(y, 0.0, 1.0) - 0.44;
    return s_curve(d + 0.44, 0.44, 1.30, 0.50);
}

fn sat01(c: vec3<f32>) -> vec3<f32> { return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)); }

// Ratio of a curved luminance to the pixel's own, 1 for black
fn lratio(t: f32, l: f32) -> f32 { return select(1.0, t / l, l > 1e-4); }

fn film_eval(sim: u32, c: vec3<f32>) -> vec3<f32> {
    let r = c.x;
    let g = c.y;
    let b = c.z;
    let luma = luma709(c);
    switch sim {
        case 1u: { // Provia
            return sat01(vec3<f32>(luma + (r - luma) * 1.05, luma + (g - luma) * 1.02, luma + (b - luma) * 1.02));
        }
        case 2u: { // Velvia
            let cc = vec3<f32>(
                s_curve((r - 0.50) + 0.50, 0.50, 1.35, 0.49),
                s_curve((g - 0.50) + 0.50, 0.50, 1.35, 0.49),
                s_curve((b - 0.50) + 0.50, 0.50, 1.35, 0.49),
            );
            let l = luma709(cc);
            return sat01(vec3<f32>(l + (cc.x - l) * 1.25, l + (cc.y - l) * 1.38, l + (cc.z - l) * 1.32));
        }
        case 3u: { // Astia
            var s = c;
            for (var i = 0u; i < 3u; i++) {
                s[i] = select(1.0 - (1.0 - c[i]) * 0.94, c[i] * 1.08, c[i] < 0.5);
            }
            let l = luma709(s);
            return sat01(vec3<f32>(l + (s.x - l) * 1.06, l + (s.y - l) * 0.98, l + (s.z - l) * 0.95));
        }
        case 4u: { // Classic Chrome
            let cl = s_curve((luma - 0.42) + 0.42, 0.42, 1.28, 0.48);
            let k = lratio(cl, luma);
            var o = c * k;
            let nl = luma709(o);
            o.x = nl + (o.x - nl) * 0.80;
            o.y = nl + (o.y - nl) * 0.84;
            o.z = nl + (o.z - nl) * 0.72 + (o.y - o.x) * 0.08;
            return sat01(o);
        }
        case 5u: { // Classic Neg
            let sc = s_curve((luma - 0.48) + 0.48, 0.48, 1.32, 0.50);
            var o = c * lratio(sc, luma);
            let nl = luma709(o);
            if (nl > 0.50) {
                let hw = (nl - 0.50) * 2.0;
                o.x += 0.03 * hw;
                o.z -= 0.02 * hw;
            } else {
                let sw = (0.50 - nl) * 2.0;
                o.y += 0.025 * sw;
                o.z += 0.015 * sw;
            }
            return sat01(o);
        }
        case 6u: { // Nostalgic Neg
            let sc = s_curve((luma - 0.45) + 0.45, 0.45, 1.22, 0.48);
            var o = c * lratio(sc, luma);
            let nl = luma709(o);
            let amber = max(nl - 0.25, 0.0) / 0.75;
            o.x += 0.045 * amber;
            o.y += 0.020 * amber;
            o.z -= 0.035 * amber;
            if (nl < 0.40) { o.z += 0.025 * ((0.40 - nl) / 0.40); }
            return sat01(o);
        }
        case 7u: { // PRO Neg. Hi
            let cl = s_curve((luma - 0.50) + 0.50, 0.50, 1.15, 0.48);
            let k = lratio(cl, luma);
            return sat01(vec3<f32>(cl + (r * k - cl) * 1.02, cl + (g * k - cl) * 0.98, cl + (b * k - cl) * 0.98));
        }
        case 8u: { // PRO Neg. Std
            let sc = s_curve((luma - 0.50) + 0.50, 0.50, 0.92, 0.50);
            let k = lratio(sc, luma);
            return sat01(vec3<f32>(sc + (r * k - sc) * 0.90, sc + (g * k - sc) * 0.90, sc + (b * k - sc) * 0.90 * 0.97));
        }
        case 9u: { // Reala Ace
            let t = s_curve((luma - 0.46) + 0.46, 0.46, 1.12, 0.50);
            let k = lratio(t, luma);
            return sat01(vec3<f32>(t + (r * k - t) * 1.04, t + (g * k - t) * 1.03, t + (b * k - t) * 1.03));
        }
        case 10u: { // Eterna
            let fl = clamp(0.46 + (luma - 0.46) * 0.75, 0.0, 1.0);
            let k = lratio(fl, luma);
            return sat01(vec3<f32>(fl + (r * k - fl) * 0.70, fl + (g * k - fl) * 0.70, fl + (b * k - fl) * 0.70));
        }
        case 11u: { // Eterna Bleach Bypass
            let hd = s_curve((luma - 0.45) + 0.45, 0.45, 1.55, 0.52);
            let k = lratio(hd, luma);
            return sat01(vec3<f32>(hd + (r * k - hd) * 0.38, hd + (g * k - hd) * 0.38, hd + (b * k - hd) * 0.38));
        }
        case 12u: { return vec3<f32>(acros_curve(0.32 * r + 0.55 * g + 0.13 * b)); }
        case 13u: { return vec3<f32>(acros_curve(0.45 * r + 0.50 * g + 0.05 * b)); }
        case 14u: { return vec3<f32>(acros_curve(0.70 * r + 0.25 * g + 0.05 * b)); }
        case 15u: { return vec3<f32>(acros_curve(0.20 * r + 0.70 * g + 0.10 * b)); }
        case 16u: { return vec3<f32>(mono_curve(0.30 * r + 0.59 * g + 0.11 * b)); }
        case 17u: { return vec3<f32>(mono_curve(0.42 * r + 0.52 * g + 0.06 * b)); }
        case 18u: { return vec3<f32>(mono_curve(0.66 * r + 0.29 * g + 0.05 * b)); }
        case 19u: { return vec3<f32>(mono_curve(0.22 * r + 0.68 * g + 0.10 * b)); }
        case 20u: { // Sepia
            let v = mono_curve(0.30 * r + 0.59 * g + 0.11 * b);
            return vec3<f32>(min(v * 1.07, 1.0), v * 0.94, v * 0.76);
        }
        case 21u: { // Hasselblad HNCS
            var hl: f32;
            if (luma < 0.50) {
                hl = 0.50 * pow(max(luma, 0.0) / 0.50, 0.96);
            } else {
                hl = 1.0 - 0.50 * pow(max(1.0 - luma, 0.0) / 0.50, 1.08);
            }
            let scale = select(1.0, pow(hl / luma, 0.25), luma > 1e-4);
            var desat = 0.0;
            if (hl > 0.85) { desat = min((hl - 0.85) / 0.15, 1.0) * 0.45; }
            return sat01(vec3<f32>(hl) + (c - vec3<f32>(luma)) * scale * (1.0 - desat));
        }
        case 22u: { // Hasselblad XPan
            let xl = s_curve((luma - 0.45) + 0.45, 0.45, 1.45, 0.52);
            let k = lratio(xl, luma);
            return sat01(vec3<f32>(xl + (r * k - xl) * 0.85, xl + (g * k - xl) * 0.85, xl + (b * k - xl) * 0.82));
        }
        default: { return c; }
    }
}

fn film(c: vec3<f32>) -> vec3<f32> {
    let sim = pu(P_FILM_SIM);
    if (sim == 0u) { return c; }
    let intensity = pf(P_FILM_INTENSITY);
    if (on(P_FILM_LUT_ON)) { return lut_apply(P_FILM_LUT, c, intensity); }
    if (intensity <= 0.001) { return c; }
    let t = film_eval(sim, c);
    if (intensity >= 0.999) { return t; }
    return c * (1.0 - intensity) + t * intensity;
}

fn color_chrome(c: vec3<f32>) -> vec3<f32> {
    let amount = pf(P_CHROME_AMOUNT);
    let blue = pf(P_CHROME_BLUE);
    let mx = max(max(c.x, c.y), c.z);
    let mn = min(min(c.x, c.y), c.z);
    if (mx <= 1e-6) { return c; }
    let sat = clamp((mx - mn) / mx, 0.0, 1.0);
    let w = sat * sat * sqrt(clamp(mx, 0.0, 1.0));
    var depth = 0.22 * clamp(amount / 100.0, 0.0, 1.0) * w;
    if (blue > 0.0) {
        let blueness = clamp((c.z - max(c.x, c.y)) / max(mx - mn, 1e-6), 0.0, 1.0);
        depth += 0.25 * clamp(blue / 100.0, 0.0, 1.0) * sat * blueness;
    }
    return c * (1.0 - min(depth, 0.4));
}

// --- ACES gamut compression (pipeline::aces) ----------------------------------------

const RGC_THRESHOLD = array<f32, 3>(0.815, 0.803, 0.880);
const RGC_LIMIT = array<f32, 3>(1.147, 1.264, 1.312);
const RGC_POWER: f32 = 1.2;

fn rgc_compress(dist: f32, thr: f32, lim: f32) -> f32 {
    if (dist < thr) { return dist; }
    let scale = (lim - thr) / pow(pow((1.0 - thr) / (lim - thr), -RGC_POWER) - 1.0, 1.0 / RGC_POWER);
    let nd = (dist - thr) / scale;
    return thr + scale * nd / pow(1.0 + pow(nd, RGC_POWER), 1.0 / RGC_POWER);
}

fn gamut_compression_encoded(c: vec3<f32>) -> vec3<f32> {
    let mx = max(max(c.x, c.y), c.z);
    let mn = min(min(c.x, c.y), c.z);
    if (mx <= 0.0 || (mx - mn) / mx < RGC_THRESHOLD[1]) { return c; }
    var o = c;
    for (var i = 0u; i < 3u; i++) {
        let d = (mx - c[i]) / mx;
        o[i] = mx - rgc_compress(d, RGC_THRESHOLD[i], RGC_LIMIT[i]) * mx;
    }
    return o;
}

// --- Entry ---------------------------------------------------------------------------

@compute @workgroup_size(16, 16)
fn point_pass(@builtin(global_invocation_id) id: vec3<u32>) {
    let w = pu(P_WIDTH);
    if (id.x >= w || id.y >= pu(P_ROWS)) { return; }
    let px = id.y * w + id.x;
    let s = px * pu(P_CHANNELS) + pu(P_SRC_SHIFT);

    // Scene-linear stage: white balance and exposure on linear light, then the
    // buffer's display curve
    let power = pf(P_POWER);
    let slope = pf(P_SLOPE);
    let toe = pf(P_TOE_LINEAR);
    let offset = pf(P_OFFSET_ENC);
    var c = vec3<f32>(
        encode_with(sample16(s) * pf(P_K + 0u), power, slope, toe, offset),
        encode_with(sample16(s + 1u) * pf(P_K + 1u), power, slope, toe, offset),
        encode_with(sample16(s + 2u) * pf(P_K + 2u), power, slope, toe, offset),
    );

    c = tone(c);
    c = curves(c);
    c = rgb_mixer(c);
    c = skin_tone(c);
    c = color_wheels(c);
    c = hsl_mixer(c);
    if (on(P_LOOK_LUT_ON)) { c = lut_apply(P_LOOK_LUT, c, pf(P_LOOK_INTENSITY)); }
    c = film(c);
    if (on(P_CHROME_ON)) { c = color_chrome(c); }
    if (on(P_ACES_ON)) { c = gamut_compression_encoded(c); }

    let o = px * 3u;
    dst[o] = c.x;
    dst[o + 1u] = c.y;
    dst[o + 2u] = c.z;
}
