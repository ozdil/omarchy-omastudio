//! XMP sidecars for exchanging edits with Lightroom, Camera Raw, Capture One,
//! darktable and digiKam.
//!
//! Export writes the Camera Raw settings (`crs:`) that have an OmaStudio equivalent,
//! the rating and colour label (`xmp:Rating`, `xmp:Label`), and the complete recipe in
//! OmaStudio's own namespace so that a round trip through another application loses
//! nothing OmaStudio set. Import prefers that recipe (while synchronizing external
//! `xmp:Rating` updates and clamping numeric fields) and otherwise maps the `crs:`
//! values back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

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

/// Strips characters illegal in XML 1.0 and escapes attribute-significant characters
/// including `\n`, `\r`, and `\t` (which XML attribute normalization would otherwise fold to spaces).
fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            '\t' => out.push_str("&#x9;"),
            // Strip illegal XML 1.0 control characters and non-characters
            '\u{0000}'..='\u{001F}' | '\u{FFFE}' | '\u{FFFF}' => {}
            _ => out.push(c),
        }
    }
    out
}

fn finite_or(v: f32, default: f32) -> f32 {
    if v.is_finite() { v } else { default }
}

fn num(v: f32) -> String {
    let v = finite_or(v, 0.0);
    let rounded = (v * 100.0).round() / 100.0;
    if rounded == rounded.trunc() { format!("{}", rounded as i64) } else { format!("{rounded}") }
}

fn signed(v: f32) -> String {
    let v = finite_or(v, 0.0);
    let s = num(v);
    if v > 0.0 && !s.starts_with('+') { format!("+{s}") } else { s }
}

/// Clamps numeric fields on a recipe decoded from an external XMP payload so tampered
/// or NaN/Inf values cannot enter the pipeline.
fn sanitize_recipe(r: &mut Recipe) {
    for (_, get, set) in FIELDS {
        let v = finite_or(get(r), 0.0);
        set(r, v);
    }
    for i in 0..8 {
        r.hsl_hue[i] = finite_or(r.hsl_hue[i], 0.0).clamp(-100.0, 100.0);
        r.hsl_sat[i] = finite_or(r.hsl_sat[i], 0.0).clamp(-100.0, 100.0);
        r.hsl_lum[i] = finite_or(r.hsl_lum[i], 0.0).clamp(-100.0, 100.0);
    }
    r.wb_temperature = finite_or(r.wb_temperature, 5500.0).clamp(1500.0, 50000.0);
    r.wb_tint = finite_or(r.wb_tint, 0.0).clamp(-150.0, 150.0);
    r.baseline_exposure = finite_or(r.baseline_exposure, 0.0).clamp(-6.0, 6.0);
    r.rotation = finite_or(r.rotation, 0.0).clamp(-360.0, 360.0);
    r.crop_x = finite_or(r.crop_x, 0.0).clamp(0.0, 1.0);
    r.crop_y = finite_or(r.crop_y, 0.0).clamp(0.0, 1.0);
    r.crop_w = finite_or(r.crop_w, 1.0).clamp(0.0, 1.0);
    r.crop_h = finite_or(r.crop_h, 1.0).clamp(0.0, 1.0);
    r.rating = r.rating.min(5);
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
        let v = finite_or(get(recipe), 0.0);
        let text = if name.starts_with("Exposure")
            || name.starts_with("Parametric")
            || matches!(*name, "Sharpness" | "LuminanceSmoothing" | "ColorNoiseReduction" | "GrainAmount")
        {
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
    let crop_x = finite_or(recipe.crop_x, 0.0).clamp(0.0, 1.0);
    let crop_y = finite_or(recipe.crop_y, 0.0).clamp(0.0, 1.0);
    let crop_w = finite_or(recipe.crop_w, 1.0).clamp(0.0, 1.0);
    let crop_h = finite_or(recipe.crop_h, 1.0).clamp(0.0, 1.0);
    let has_crop = crop_w > 0.0 && crop_h > 0.0 && (crop_w < 1.0 || crop_h < 1.0 || crop_x > 0.0 || crop_y > 0.0);
    // Camera Raw's CropAngle is a straightening angle (at most 45°); quarter turns
    // only travel in the OmaStudio recipe
    let rot = finite_or(recipe.rotation, 0.0);
    let straighten = rot - (rot / 90.0).round() * 90.0;
    if has_crop || straighten.abs() >= 0.01 {
        let (w, h) = if crop_w > 0.0 && crop_h > 0.0 { (crop_w, crop_h) } else { (1.0, 1.0) };
        crs.push(("CropTop".into(), format!("{:.6}", crop_y)));
        crs.push(("CropLeft".into(), format!("{:.6}", crop_x)));
        crs.push(("CropBottom".into(), format!("{:.6}", (crop_y + h).min(1.0))));
        crs.push(("CropRight".into(), format!("{:.6}", (crop_x + w).min(1.0))));
        crs.push(("CropAngle".into(), num(straighten)));
        crs.push(("HasCrop".into(), "True".into()));
    }
    crs.push(("HasSettings".into(), "True".into()));

    let mut clean_recipe = recipe.clone();
    sanitize_recipe(&mut clean_recipe);
    let recipe_json = serde_json::to_string(&clean_recipe).ok();

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
        out.push_str(&format!("   xmp:Label=\"{}\"\n", escape_attr(&culling.label)));
    }
    for (k, v) in &crs {
        out.push_str(&format!("   crs:{k}=\"{}\"\n", escape_attr(v)));
    }
    if let Some(json) = recipe_json {
        out.push_str(&format!("   omastudio:Recipe=\"{}\">\n", escape_attr(&json)));
    } else {
        out.push_str("  >\n");
    }
    for (name, points) in curve_fields(recipe) {
        let valid: Vec<(i32, i32)> = points
            .iter()
            .filter(|p| p[0].is_finite() && p[1].is_finite())
            .map(|p| ((p[0].clamp(0.0, 1.0) * 255.0).round() as i32, (p[1].clamp(0.0, 1.0) * 255.0).round() as i32))
            .collect();
        if valid.is_empty() {
            continue;
        }
        out.push_str(&format!("   <crs:{name}>\n    <rdf:Seq>\n"));
        for (x, y) in valid {
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

struct ParsedXmp {
    props: HashMap<String, String>,
    curves: HashMap<String, Vec<[f32; 2]>>,
}

impl ParsedXmp {
    fn parse(xmp: &str) -> Self {
        let mut reader = Reader::from_str(xmp);
        // XMP packets often contain partial/hand-crafted fragments or comments;
        // check_end_names(false) stays resilient while still skipping comments and decoding entities.
        reader.config_mut().check_end_names = false;

        let mut props: HashMap<String, String> = HashMap::new();
        let mut curves: HashMap<String, Vec<[f32; 2]>> = HashMap::new();
        let mut stack: Vec<String> = Vec::new();
        let mut text_buf = String::new();
        let mut active_curve: Option<String> = None;

        let is_curve_elem = |name: &str| {
            matches!(
                name,
                "crs:ToneCurvePV2012" | "crs:ToneCurvePV2012Red" | "crs:ToneCurvePV2012Green" | "crs:ToneCurvePV2012Blue"
            )
        };

        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    let qname = e.name().as_ref().to_string();
                    for attr in e.attributes().with_checks(false).flatten() {
                        let k = attr.key.as_ref().to_string();
                        if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                            props.entry(k).or_insert_with(|| v.into_owned());
                        }
                    }
                    if is_curve_elem(&qname) {
                        active_curve = Some(qname.clone());
                    }
                    stack.push(qname);
                    text_buf.clear();
                }
                Ok(Event::Empty(e)) => {
                    for attr in e.attributes().with_checks(false).flatten() {
                        let k = attr.key.as_ref().to_string();
                        if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                            props.entry(k).or_insert_with(|| v.into_owned());
                        }
                    }
                }
                Ok(Event::Text(e)) => {
                    text_buf.push_str(&e.xml_content(XmlVersion::Implicit1_0));
                }
                Ok(Event::CData(e)) => {
                    text_buf.push_str(&e.xml_content(XmlVersion::Implicit1_0));
                }
                Ok(Event::GeneralRef(e)) => {
                    if let Ok(Some(ch)) = e.resolve_char_ref() {
                        text_buf.push(ch);
                    } else {
                        match e.as_ref() {
                            "amp" => text_buf.push('&'),
                            "lt" => text_buf.push('<'),
                            "gt" => text_buf.push('>'),
                            "quot" => text_buf.push('"'),
                            "apos" => text_buf.push('\''),
                            other => {
                                text_buf.push('&');
                                text_buf.push_str(other);
                                text_buf.push(';');
                            }
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    let qname = e.name().as_ref().to_string();
                    let trimmed = text_buf.trim();
                    if qname == "rdf:li"
                        && let Some(ref cname) = active_curve
                    {
                        if let Some((xs, ys)) = trimmed.split_once(',')
                            && let (Ok(x), Ok(y)) = (xs.trim().parse::<f32>(), ys.trim().parse::<f32>())
                            && x.is_finite()
                            && y.is_finite()
                        {
                            curves
                                .entry(cname.clone())
                                .or_default()
                                .push([(x / 255.0).clamp(0.0, 1.0), (y / 255.0).clamp(0.0, 1.0)]);
                        }
                    } else if !trimmed.is_empty() && !matches!(qname.as_str(), "rdf:Description" | "rdf:RDF" | "rdf:Seq" | "x:xmpmeta") {
                        props.entry(qname.clone()).or_insert_with(|| trimmed.to_string());
                    }
                    if active_curve.as_deref() == Some(qname.as_str()) {
                        active_curve = None;
                    }
                    stack.pop();
                    text_buf.clear();
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Self { props, curves }
    }

    fn property(&self, qname: &str) -> Option<&str> {
        self.props.get(qname).map(String::as_str)
    }

    fn number(&self, qname: &str) -> Option<f32> {
        self.property(qname)?
            .trim()
            .trim_start_matches('+')
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
    }

    fn curve(&self, qname: &str) -> Vec<[f32; 2]> {
        self.curves.get(qname).cloned().unwrap_or_default()
    }
}

/// Reads a recipe and culling data from an XMP packet.
pub fn parse(xmp: &str) -> (Recipe, Culling) {
    let doc = ParsedXmp::parse(xmp);
    let rating_opt = doc.number("xmp:Rating").map(|r| r.round() as i32);
    let culling = Culling {
        rating: rating_opt.unwrap_or(0).clamp(-1, 5),
        label: doc.property("xmp:Label").unwrap_or_default().to_string(),
    };
    if let Some(mut recipe) = doc
        .property("omastudio:Recipe")
        .and_then(|j| serde_json::from_str::<Recipe>(j).ok())
    {
        sanitize_recipe(&mut recipe);
        // Synchronize external xmp:Rating updates made in Lightroom/digiKam
        if let Some(ext_rating) = rating_opt {
            let clamped = ext_rating.clamp(-1, 5);
            recipe.rating = clamped.clamp(0, 5) as u8;
            if clamped < 0 {
                recipe.flag = "reject".to_string();
            } else if recipe.flag == "reject" && clamped > 0 {
                recipe.flag = "none".to_string();
            }
        }
        return (recipe, culling);
    }
    let mut r = Recipe::default();
    for (name, _, set) in FIELDS {
        if let Some(v) = doc.number(&format!("crs:{name}")) {
            set(&mut r, v);
        }
    }
    for (i, color) in HSL_NAMES.iter().enumerate() {
        if let Some(v) = doc.number(&format!("crs:HueAdjustment{color}")) {
            r.hsl_hue[i] = v.clamp(-100.0, 100.0);
        }
        if let Some(v) = doc.number(&format!("crs:SaturationAdjustment{color}")) {
            r.hsl_sat[i] = v.clamp(-100.0, 100.0);
        }
        if let Some(v) = doc.number(&format!("crs:LuminanceAdjustment{color}")) {
            r.hsl_lum[i] = v.clamp(-100.0, 100.0);
        }
    }
    r.curve_points = doc.curve("crs:ToneCurvePV2012");
    r.curve_points_red = doc.curve("crs:ToneCurvePV2012Red");
    r.curve_points_green = doc.curve("crs:ToneCurvePV2012Green");
    r.curve_points_blue = doc.curve("crs:ToneCurvePV2012Blue");
    if doc.property("crs:HasCrop").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        let side = |n: &str| doc.number(&format!("crs:Crop{n}")).map(|v| v.clamp(0.0, 1.0));
        if let (Some(t), Some(l), Some(b), Some(rt)) = (side("Top"), side("Left"), side("Bottom"), side("Right"))
            && b > t
            && rt > l
        {
            r.crop_x = l;
            r.crop_y = t;
            r.crop_w = rt - l;
            r.crop_h = b - t;
        }
        if let Some(a) = doc.number("crs:CropAngle") {
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

/// Writes the sidecar. An XMP that another application wrote — or any existing
/// unreadable, >4 MiB, or symlink XMP file — is only replaced with `force`.
pub fn export(photo: &Path, recipe: &Recipe, culling: &Culling, out: Option<&Path>, force: bool) -> Result<PathBuf, String> {
    let path = out.map(Path::to_path_buf).unwrap_or_else(|| sidecar_path(photo));
    if !force {
        match crate::security::read_secure_file(&path, MAX_XMP) {
            Ok(existing) => {
                if !String::from_utf8_lossy(&existing).contains(NS_OMASTUDIO) {
                    return Err(format!("{} was written by another application; pass --force to replace it", path.display()));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(format!(
                    "{} already exists and cannot be verified as an OmaStudio sidecar ({e}); pass --force to replace it",
                    path.display()
                ));
            }
        }
    }
    crate::security::atomic_write_user(&path, write(recipe, culling).as_bytes()).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_the_whole_recipe() {
        let mut r = Recipe {
            exposure: 0.7,
            contrast: -12.0,
            rating: 4,
            film_simulation: "fuji_classic_chrome".into(),
            ..Recipe::default()
        };
        r.hsl_sat[5] = -20.0;
        let culling = Culling { rating: 4, label: "Green & \"Blue\"\nLine2".into() };
        let xml = write(&r, &culling);
        assert!(xml.contains("&#xA;"), "newlines in XML attributes must be escaped");
        let (back, c) = parse(&xml);
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
    fn numeric_entities_and_xml_comments_are_handled_correctly() {
        // Numeric entities &#34; (") and &#xA; (\n) emitted by Lightroom/ExifTool,
        // plus a spoofed attribute inside an XML comment that must be ignored.
        let r = Recipe { exposure: 1.5, preset_name: Some("Line1\nLine2".into()), ..Recipe::default() };
        let encoded = write(&r, &Culling { rating: 3, label: "Red".into() })
            .replace("&quot;", "&#34;");
        let with_comment = format!("<!-- crs:Exposure2012=\"-5.0\" xmp:Rating=\"0\" -->\n{encoded}");
        let (back, c) = parse(&with_comment);
        assert_eq!(back.exposure, 1.5);
        assert_eq!(back.preset_name.as_deref(), Some("Line1\nLine2"));
        assert_eq!(c.rating, 3);
        assert_eq!(back.rating, 3);
    }

    #[test]
    fn external_xmp_rating_syncs_with_embedded_recipe_and_clamps_fields() {
        let xmp = r#"<rdf:Description rdf:about="" xmp:Rating="5"
            omastudio:Recipe="{&quot;exposure&quot;:99.0,&quot;rating&quot;:1}"/>"#;
        let (r, c) = parse(xmp);
        assert_eq!(c.rating, 5);
        assert_eq!(r.rating, 5);
        assert_eq!(r.exposure, 5.0);
    }

    #[test]
    fn write_strips_xml_control_chars_and_handles_nan() {
        let r = Recipe {
            exposure: f32::NAN,
            rotation: f32::INFINITY,
            preset_name: Some("ok\u{0000}\u{0007}end".into()),
            ..Recipe::default()
        };
        let xml = write(&r, &Culling { rating: 0, label: "a\u{0000}b".into() });
        assert!(!xml.contains("NaN"));
        assert!(!xml.contains('\0'));
        assert!(!xml.contains('\u{0007}'));
    }

    #[test]
    fn export_refuses_to_overwrite_symlink_or_oversized_foreign_xmp_without_force() {
        let dir = std::env::temp_dir().join(format!("omastudio_xmp_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("DSCF0001.RAF");
        std::fs::write(&photo, b"raw").unwrap();
        let foreign = dir.join("foreign.xmp");
        std::fs::write(&foreign, b"<x/>").unwrap();
        let xmp_link = dir.join("DSCF0001.xmp");
        std::os::unix::fs::symlink(&foreign, &xmp_link).unwrap();

        let err = export(&photo, &Recipe::default(), &Culling::default(), None, false)
            .expect_err("must refuse overwriting existing symlink XMP without --force");
        assert!(err.contains("--force"), "unexpected error: {err}");

        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(ParsedXmp::parse(&xmp).number("crs:CropAngle"), Some(1.5));
        let r = Recipe { rotation: 90.0, ..Recipe::default() };
        assert!(!write(&r, &Culling::default()).contains("crs:CropAngle"));
    }

    #[test]
    fn names_must_match_whole() {
        // crs:Saturation must not pick up crs:SaturationAdjustmentRed
        let xmp = r#"<x crs:SaturationAdjustmentRed="50" crs:Saturation="-10"/>"#;
        assert_eq!(ParsedXmp::parse(xmp).number("crs:Saturation"), Some(-10.0));
    }
}
