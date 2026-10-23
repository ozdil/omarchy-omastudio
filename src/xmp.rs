//! XMP sidecars for exchanging edits with Lightroom, Camera Raw, Capture One,
//! darktable and digiKam.
//!
//! Export writes the Camera Raw settings (`crs:`) that have an OmaStudio equivalent,
//! the rating and colour label (`xmp:Rating`, `xmp:Label`), and the complete recipe in
//! OmaStudio's own namespace so that a round trip through another application loses
//! nothing OmaStudio set. Import prefers that recipe and otherwise maps the `crs:`
//! values back.

use std::path::{Path, PathBuf};

use crate::recipe::Recipe;

const NS_OMASTUDIO: &str = "https://github.com/ozdil/omarchy-omastudio/xmp/1.0/";
const MAX_XMP: usize = 4 << 20;

const HSL_NAMES: [&str; 8] = ["Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"];

/// One-to-one numeric settings: (crs name, getter, setter)
type Field = (&'static str, fn(&Recipe) -> f32, fn(&mut Recipe, f32));
const FIELDS: &[Field] = &[
    ("Exposure2012", |r| r.exposure, |r, v| r.exposure = v.clamp(-5.0, 5.0)),
    ("Contrast2012", |r| r.contrast, |r, v| r.contrast = v.clamp(-100.0, 100.0)),
    ("Highlights2012", |r| r.highlights, |r, v| r.highlights = v.clamp(-100.0, 100.0)),
    ("Shadows2012", |r| r.shadows, |r, v| r.shadows = v.clamp(-100.0, 100.0)),
    ("Whites2012", |r| r.whites, |r, v| r.whites = v.clamp(-100.0, 100.0)),
    ("Blacks2012", |r| r.blacks, |r, v| r.blacks = v.clamp(-100.0, 100.0)),
    ("Texture", |r| r.texture, |r, v| r.texture = v.clamp(-100.0, 100.0)),
    ("Clarity2012", |r| r.clarity, |r, v| r.clarity = v.clamp(-100.0, 100.0)),
    ("Dehaze", |r| r.dehaze, |r, v| r.dehaze = v.clamp(-100.0, 100.0)),
    ("Vibrance", |r| r.vibrance, |r, v| r.vibrance = v.clamp(-100.0, 100.0)),
    ("Saturation", |r| r.saturation, |r, v| r.saturation = v.clamp(-100.0, 100.0)),
    ("ParametricHighlights", |r| r.curve_highlights, |r, v| r.curve_highlights = v.clamp(-100.0, 100.0)),
    ("ParametricLights", |r| r.curve_lights, |r, v| r.curve_lights = v.clamp(-100.0, 100.0)),
    ("ParametricDarks", |r| r.curve_darks, |r, v| r.curve_darks = v.clamp(-100.0, 100.0)),
    ("ParametricShadows", |r| r.curve_shadows, |r, v| r.curve_shadows = v.clamp(-100.0, 100.0)),
    ("Sharpness", |r| r.sharpness, |r, v| r.sharpness = v.clamp(0.0, 100.0)),
    ("LuminanceSmoothing", |r| r.denoise_lum, |r, v| r.denoise_lum = v.clamp(0.0, 100.0)),
    ("ColorNoiseReduction", |r| r.denoise_col, |r, v| r.denoise_col = v.clamp(0.0, 100.0)),
    ("PostCropVignetteAmount", |r| r.vignette, |r, v| r.vignette = v.clamp(-100.0, 100.0)),
    ("GrainAmount", |r| r.grain_amount, |r, v| r.grain_amount = v.clamp(0.0, 100.0)),
];

/// Rating, label and flag that travel with the edit.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Culling {
    /// 0-5; Lightroom writes -1 for rejected photos
    pub rating: i32,
    pub label: String,
}

/// The sidecar path other applications use: `DSCF0001.xmp` next to `DSCF0001.RAF`.
pub fn sidecar_path(photo: &Path) -> PathBuf {
    photo.with_extension("xmp")
}

/// darktable's sidecar name, `DSCF0001.RAF.xmp`, read when the other is missing.
fn darktable_path(photo: &Path) -> PathBuf {
    let mut name = photo.as_os_str().to_owned();
    name.push(".xmp");
    PathBuf::from(name)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn num(v: f32) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    if rounded == rounded.trunc() { format!("{}", rounded as i64) } else { format!("{rounded}") }
}

fn signed(v: f32) -> String {
    let s = num(v);
    if v > 0.0 && !s.starts_with('+') { format!("+{s}") } else { s }
}

/// Builds the XMP packet.
pub fn write(recipe: &Recipe, culling: &Culling) -> String {
    let mut crs: Vec<(String, String)> = vec![
        ("Version".into(), "15.0".into()),
        ("ProcessVersion".into(), "11.0".into()),
    ];
    // OmaStudio's white balance is relative to the camera's: only "as shot" maps exactly
    if recipe.wb_temperature == 5500.0 && recipe.wb_tint == 0.0 {
        crs.push(("WhiteBalance".into(), "As Shot".into()));
    }
    for (name, get, _) in FIELDS {
        let v = get(recipe);
        let text = if name.starts_with("Exposure") || name.starts_with("Parametric") || matches!(*name, "Sharpness" | "LuminanceSmoothing" | "ColorNoiseReduction" | "GrainAmount") {
            num(v)
        } else {
            signed(v)
        };
        crs.push(((*name).into(), text));
    }
    for (i, color) in HSL_NAMES.iter().enumerate() {
        crs.push((format!("HueAdjustment{color}"), num(recipe.hsl_hue[i])));
        crs.push((format!("SaturationAdjustment{color}"), num(recipe.hsl_sat[i])));
        crs.push((format!("LuminanceAdjustment{color}"), num(recipe.hsl_lum[i])));
    }
    let has_crop = recipe.crop_w > 0.0 && recipe.crop_h > 0.0 && (recipe.crop_w < 1.0 || recipe.crop_h < 1.0 || recipe.crop_x > 0.0 || recipe.crop_y > 0.0);
    // Camera Raw's CropAngle is a straightening angle (at most 45°); quarter turns
    // only travel in the OmaStudio recipe
    let straighten = recipe.rotation - (recipe.rotation / 90.0).round() * 90.0;
    if has_crop || straighten.abs() >= 0.01 {
        let (w, h) = if recipe.crop_w > 0.0 && recipe.crop_h > 0.0 { (recipe.crop_w, recipe.crop_h) } else { (1.0, 1.0) };
        crs.push(("CropTop".into(), format!("{:.6}", recipe.crop_y)));
        crs.push(("CropLeft".into(), format!("{:.6}", recipe.crop_x)));
        crs.push(("CropBottom".into(), format!("{:.6}", (recipe.crop_y + h).min(1.0))));
        crs.push(("CropRight".into(), format!("{:.6}", (recipe.crop_x + w).min(1.0))));
        crs.push(("CropAngle".into(), num(straighten)));
        crs.push(("HasCrop".into(), "True".into()));
    }
    crs.push(("HasSettings".into(), "True".into()));

    let recipe_json = serde_json::to_string(recipe).unwrap_or_default();
    let mut out = String::new();
    out.push_str("<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"OmaStudio\">\n");
    out.push_str(" <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    out.push_str("  <rdf:Description rdf:about=\"\"\n");
    out.push_str("    xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\"\n");
    out.push_str("    xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\"\n");
    out.push_str(&format!("    xmlns:omastudio=\"{NS_OMASTUDIO}\"\n"));
    out.push_str(&format!("   xmp:Rating=\"{}\"\n", culling.rating.clamp(-1, 5)));
    if !culling.label.is_empty() {
        out.push_str(&format!("   xmp:Label=\"{}\"\n", escape(&culling.label)));
    }
    for (k, v) in &crs {
        out.push_str(&format!("   crs:{k}=\"{}\"\n", escape(v)));
    }
    out.push_str(&format!("   omastudio:Recipe=\"{}\">\n", escape(&recipe_json)));
    for (name, points) in curve_fields(recipe) {
        if points.is_empty() {
            continue;
        }
        out.push_str(&format!("   <crs:{name}>\n    <rdf:Seq>\n"));
        for p in points {
            let (x, y) = ((p[0] * 255.0).round() as i32, (p[1] * 255.0).round() as i32);
            out.push_str(&format!("     <rdf:li>{x}, {y}</rdf:li>\n"));
        }
        out.push_str(&format!("    </rdf:Seq>\n   </crs:{name}>\n"));
    }
    out.push_str("  </rdf:Description>\n");
    out.push_str(" </rdf:RDF>\n</x:xmpmeta>\n");
    out.push_str("<?xpacket end=\"w\"?>\n");
    out
}

/// Point curves in Camera Raw's names (points on 0..255).
fn curve_fields(r: &Recipe) -> [(&'static str, &Vec<[f32; 2]>); 4] {
    [
        ("ToneCurvePV2012", &r.curve_points),
        ("ToneCurvePV2012Red", &r.curve_points_red),
        ("ToneCurvePV2012Green", &r.curve_points_green),
        ("ToneCurvePV2012Blue", &r.curve_points_blue),
    ]
}

/// The points of an `rdf:Seq` curve element such as `crs:ToneCurvePV2012`, scaled to 0..1.
fn curve(xmp: &str, qname: &str) -> Vec<[f32; 2]> {
    let open = format!("<{qname}>");
    let Some(start) = xmp.find(&open).map(|i| i + open.len()) else {
        return Vec::new();
    };
    let Some(len) = xmp[start..].find(&format!("</{qname}>")) else {
        return Vec::new();
    };
    xmp[start..start + len]
        .split("<rdf:li>")
        .skip(1)
        .filter_map(|li| {
            let item = li.split("</rdf:li>").next()?;
            let (x, y) = item.split_once(',')?;
            let (x, y) = (x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?);
            (x.is_finite() && y.is_finite()).then(|| [(x / 255.0).clamp(0.0, 1.0), (y / 255.0).clamp(0.0, 1.0)])
        })
        .collect()
}

/// The value of `prefix:name`, written as an attribute or as an element.
fn property(xmp: &str, qname: &str) -> Option<String> {
    let attr = format!("{qname}=");
    let mut from = 0;
    while let Some(pos) = xmp[from..].find(&attr) {
        let start = from + pos;
        from = start + attr.len();
        // Must be a whole name: preceded by whitespace
        if !xmp[..start].ends_with(|c: char| c.is_whitespace()) {
            continue;
        }
        let rest = &xmp[from..];
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let end = rest[1..].find(quote)?;
        return Some(unescape(&rest[1..1 + end]));
    }
    let open = format!("<{qname}>");
    let start = xmp.find(&open)? + open.len();
    let end = xmp[start..].find(&format!("</{qname}>"))?;
    Some(unescape(xmp[start..start + end].trim()))
}

fn number(xmp: &str, qname: &str) -> Option<f32> {
    property(xmp, qname)?.trim().trim_start_matches('+').parse::<f32>().ok().filter(|v| v.is_finite())
}

/// Reads a recipe and culling data from an XMP packet.
pub fn parse(xmp: &str) -> (Recipe, Culling) {
    let culling = Culling {
        rating: number(xmp, "xmp:Rating").map(|r| r.round() as i32).unwrap_or(0).clamp(-1, 5),
        label: property(xmp, "xmp:Label").unwrap_or_default(),
    };
    if let Some(recipe) = property(xmp, "omastudio:Recipe").and_then(|j| serde_json::from_str::<Recipe>(&j).ok()) {
        return (recipe, culling);
    }
    let mut r = Recipe::default();
    for (name, _, set) in FIELDS {
        if let Some(v) = number(xmp, &format!("crs:{name}")) {
            set(&mut r, v);
        }
    }
    for (i, color) in HSL_NAMES.iter().enumerate() {
        if let Some(v) = number(xmp, &format!("crs:HueAdjustment{color}")) {
            r.hsl_hue[i] = v.clamp(-100.0, 100.0);
        }
        if let Some(v) = number(xmp, &format!("crs:SaturationAdjustment{color}")) {
            r.hsl_sat[i] = v.clamp(-100.0, 100.0);
        }
        if let Some(v) = number(xmp, &format!("crs:LuminanceAdjustment{color}")) {
            r.hsl_lum[i] = v.clamp(-100.0, 100.0);
        }
    }
    r.curve_points = curve(xmp, "crs:ToneCurvePV2012");
    r.curve_points_red = curve(xmp, "crs:ToneCurvePV2012Red");
    r.curve_points_green = curve(xmp, "crs:ToneCurvePV2012Green");
    r.curve_points_blue = curve(xmp, "crs:ToneCurvePV2012Blue");
    if property(xmp, "crs:HasCrop").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        let side = |n: &str| number(xmp, &format!("crs:Crop{n}")).map(|v| v.clamp(0.0, 1.0));
        if let (Some(t), Some(l), Some(b), Some(rt)) = (side("Top"), side("Left"), side("Bottom"), side("Right"))
            && b > t
            && rt > l
        {
            r.crop_x = l;
            r.crop_y = t;
            r.crop_w = rt - l;
            r.crop_h = b - t;
        }
        if let Some(a) = number(xmp, "crs:CropAngle") {
            r.rotation = a.clamp(-45.0, 45.0);
        }
    }
    r.rating = culling.rating.clamp(0, 5) as u8;
    if culling.rating < 0 {
        r.flag = "reject".to_string();
    }
    (r, culling)
}

/// Reads the sidecar of a photo (`photo.xmp`, else `photo.RAF.xmp`) or the given file.
pub fn read(photo: &Path, from: Option<&Path>) -> Result<(Recipe, Culling, PathBuf), String> {
    let candidates: Vec<PathBuf> = match from {
        Some(f) => vec![f.to_path_buf()],
        None => vec![sidecar_path(photo), darktable_path(photo)],
    };
    for path in candidates {
        if let Ok(bytes) = crate::security::read_secure_file(&path, MAX_XMP) {
            let (recipe, culling) = parse(&String::from_utf8_lossy(&bytes));
            return Ok((recipe, culling, path));
        }
    }
    Err(format!("No XMP sidecar found for {}", photo.display()))
}

/// Writes the sidecar. An XMP that another application wrote is only replaced with `force`.
pub fn export(photo: &Path, recipe: &Recipe, culling: &Culling, out: Option<&Path>, force: bool) -> Result<PathBuf, String> {
    let path = out.map(Path::to_path_buf).unwrap_or_else(|| sidecar_path(photo));
    if !force
        && let Ok(existing) = crate::security::read_secure_file(&path, MAX_XMP)
        && !String::from_utf8_lossy(&existing).contains(NS_OMASTUDIO)
    {
        return Err(format!("{} was written by another application; pass --force to replace it", path.display()));
    }
    crate::security::atomic_write_user(&path, write(recipe, culling).as_bytes()).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_the_whole_recipe() {
        let mut r = Recipe { exposure: 0.7, contrast: -12.0, film_simulation: "fuji_classic_chrome".into(), ..Recipe::default() };
        r.hsl_sat[5] = -20.0;
        let culling = Culling { rating: 4, label: "Green & \"Blue\"".into() };
        let (back, c) = parse(&write(&r, &culling));
        assert_eq!(back, r);
        assert_eq!(c, culling);
    }

    #[test]
    fn reads_camera_raw_settings() {
        let xmp = r#"<rdf:Description rdf:about="" xmp:Rating="-1"
            crs:Exposure2012="+1.25" crs:Contrast2012="-20" crs:SaturationAdjustmentBlue="-30"
            crs:HasCrop="True" crs:CropTop="0.1" crs:CropLeft="0.2" crs:CropBottom="0.9" crs:CropRight="0.8" crs:CropAngle="1.5">
            <crs:Clarity2012>+15</crs:Clarity2012></rdf:Description>"#;
        let (r, c) = parse(xmp);
        assert_eq!(r.exposure, 1.25);
        assert_eq!(r.contrast, -20.0);
        assert_eq!(r.clarity, 15.0);
        assert_eq!(r.hsl_sat[5], -30.0);
        assert!((r.crop_w - 0.6).abs() < 1e-6 && (r.crop_h - 0.8).abs() < 1e-6);
        assert_eq!(r.rotation, 1.5);
        assert_eq!(c.rating, -1);
        assert_eq!(r.flag, "reject");
    }

    #[test]
    fn point_curves_use_camera_raw_names() {
        let r = Recipe {
            curve_points: vec![[0.0, 0.0], [0.25, 0.2], [1.0, 1.0]],
            curve_points_blue: vec![[0.0, 0.1], [1.0, 1.0]],
            ..Recipe::default()
        };
        let xmp = write(&r, &Culling::default());
        assert!(xmp.contains("<crs:ToneCurvePV2012>") && xmp.contains("<rdf:li>64, 51</rdf:li>"));
        assert!(!xmp.contains("ToneCurvePV2012Red"));
        // Without our own recipe the Camera Raw curves are read
        let foreign = xmp.replace("omastudio:Recipe=", "omastudio:Other=");
        let (back, _) = parse(&foreign);
        assert_eq!(back.curve_points.len(), 3);
        assert!((back.curve_points[1][0] - 64.0 / 255.0).abs() < 1e-6);
        assert!((back.curve_points_blue[0][1] - 26.0 / 255.0).abs() < 1e-6);
        assert!(back.curve_points_red.is_empty());
    }

    #[test]
    fn quarter_turns_are_not_written_as_crop_angle() {
        let r = Recipe { rotation: 91.5, ..Recipe::default() };
        let xmp = write(&r, &Culling::default());
        assert_eq!(number(&xmp, "crs:CropAngle"), Some(1.5));
        let r = Recipe { rotation: 90.0, ..Recipe::default() };
        assert!(!write(&r, &Culling::default()).contains("crs:CropAngle"));
    }

    #[test]
    fn names_must_match_whole() {
        // crs:Saturation must not pick up crs:SaturationAdjustmentRed
        let xmp = r#"<x crs:SaturationAdjustmentRed="50" crs:Saturation="-10"/>"#;
        assert_eq!(number(xmp, "crs:Saturation"), Some(-10.0));
    }
}
