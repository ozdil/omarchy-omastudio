//! X RAW Studio conversion profiles (.FP1, .FP2, .FP3).
//!
//! A profile is a flat XML file: `<ConversionProfile>` holding one `<PropertyGroup
//! device=".." label="..">` whose child elements are the camera menu settings by name
//! (`<FilmSimulation>Classic</FilmSimulation>`, `<GrainEffect>STRONG</GrainEffect>`...).
//! Value spellings follow profiles saved by X RAW Studio 1.x.

use std::collections::HashMap;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use super::{CameraRecipe, FilmSim, GrainSize, Strength};

/// Profiles are a few KiB; anything larger is not one.
pub const MAX_SIZE: usize = 256 * 1024;

/// A conversion profile: its name, the camera it was made for and the recipe.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    pub label: String,
    pub device: String,
    pub recipe: CameraRecipe,
}

/// (FP1 name, film simulation). The first name for a simulation is the one written.
const FILM_SIMS: &[(&str, FilmSim)] = &[
    ("Provia", FilmSim::Provia),
    ("Velvia", FilmSim::Velvia),
    ("Astia", FilmSim::Astia),
    ("Classic", FilmSim::ClassicChrome),
    ("ClassicNEGA", FilmSim::ClassicNeg),
    ("NEGAStd", FilmSim::ProNegStd),
    ("NEGAhi", FilmSim::ProNegHi),
    ("Eterna", FilmSim::Eterna),
    ("BleachBypass", FilmSim::BleachBypass),
    ("NostalgicNEGA", FilmSim::NostalgicNeg),
    // Not seen in a saved profile yet; spelled like the other names
    ("RealaACE", FilmSim::RealaAce),
    ("Acros", FilmSim::Acros),
    ("AcrosYe", FilmSim::AcrosYellow),
    ("AcrosR", FilmSim::AcrosRed),
    ("AcrosG", FilmSim::AcrosGreen),
    ("BW", FilmSim::Monochrome),
    ("BYe", FilmSim::MonochromeYellow),
    ("BR", FilmSim::MonochromeRed),
    ("BG", FilmSim::MonochromeGreen),
    ("Sepia", FilmSim::Sepia),
];

/// (FP1 white balance, CameraRecipe white balance name)
const WHITE_BALANCES: &[(&str, &str)] = &[
    ("Auto", "Auto"),
    ("Auto_White", "Auto (white priority)"),
    ("AutoWhitePriority", "Auto (white priority)"),
    ("Auto_Ambience", "Auto (ambience priority)"),
    ("AutoAmbiencePriority", "Auto (ambience priority)"),
    ("Daylight", "Daylight"),
    ("Shade", "Shade"),
    ("FLight1", "Fluorescent"),
    ("FLight2", "Fluorescent"),
    ("FLight3", "Fluorescent"),
    ("Incandescent", "Incandescent"),
    ("UnderWater", "Underwater"),
    ("Custom1", "Custom"),
    ("Custom2", "Custom"),
    ("Custom3", "Custom"),
    ("Temperature", "Kelvin"),
];

/// Escapes XML 1.0 text/attribute characters and strips illegal control characters.
fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            '\u{0000}'..='\u{001F}' | '\u{FFFE}' | '\u{FFFF}' => {}
            _ => out.push(c),
        }
    }
    out
}

fn strength(v: &str) -> Strength {
    match v.to_ascii_uppercase().as_str() {
        "WEAK" => Strength::Weak,
        "STRONG" => Strength::Strong,
        _ => Strength::Off,
    }
}

fn strength_name(s: Strength) -> &'static str {
    match s {
        Strength::Off => "OFF",
        Strength::Weak => "WEAK",
        Strength::Strong => "STRONG",
    }
}

/// "P1P67" = +1 2/3 EV, "M0P33" = -1/3 EV, "0" = 0.
fn parse_ev(v: &str) -> f32 {
    let (sign, rest) = match v.as_bytes().first() {
        Some(b'P') => (1.0, &v[1..]),
        Some(b'M') => (-1.0, &v[1..]),
        _ => {
            let parsed = v.parse::<f32>().unwrap_or(0.0);
            return if parsed.is_finite() { parsed.clamp(-5.0, 5.0) } else { 0.0 };
        }
    };
    let value: f32 = rest.replacen('P', ".", 1).parse().unwrap_or(0.0);
    if !value.is_finite() {
        return 0.0;
    }
    // .33/.67 are thirds of a stop
    (sign * ((value.clamp(0.0, 5.0) * 3.0).round() / 3.0)).clamp(-5.0, 5.0)
}

fn format_ev(ev: f32) -> String {
    if !ev.is_finite() {
        return "0".to_string();
    }
    let thirds = (ev.clamp(-5.0, 5.0) * 3.0).round() as i32;
    if thirds == 0 {
        return "0".to_string();
    }
    let sign = if thirds > 0 { 'P' } else { 'M' };
    let abs = thirds.unsigned_abs();
    let (whole, frac) = (abs / 3, abs % 3);
    let frac = ["00", "33", "67"][frac as usize];
    format!("{}{}P{}", sign, whole, frac)
}

fn int_clamped(v: &str, min: i32, max: i32) -> i32 {
    match v.trim().trim_start_matches('+').parse::<f32>() {
        Ok(f) if f.is_finite() => (f.round() as i32).clamp(min, max),
        _ => 0,
    }
}

fn tone_clamped(v: &str) -> f32 {
    match v.trim().parse::<f32>() {
        Ok(f) if f.is_finite() => f.clamp(-2.0, 4.0),
        _ => 0.0,
    }
}

struct ParsedFp1 {
    has_conversion_profile: bool,
    has_property_group: bool,
    device: String,
    label: String,
    fields: HashMap<String, String>,
}

fn parse_fp1_xml(xml: &str) -> Result<ParsedFp1, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().check_end_names = false;

    let mut parsed = ParsedFp1 {
        has_conversion_profile: false,
        has_property_group: false,
        device: String::new(),
        label: String::new(),
        fields: HashMap::new(),
    };
    let mut in_first_group = false;
    let mut group_done = false;
    let mut current_field: Option<String> = None;
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_string();
                if name == "ConversionProfile" {
                    parsed.has_conversion_profile = true;
                } else if name == "PropertyGroup" && !group_done {
                    parsed.has_property_group = true;
                    in_first_group = true;
                    for attr in e.attributes().with_checks(false).flatten() {
                        let k = attr.key.as_ref();
                        if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                            match k {
                                "device" => parsed.device = v.into_owned(),
                                "label" => parsed.label = v.into_owned(),
                                _ => {}
                            }
                        }
                    }
                } else if in_first_group {
                    current_field = Some(name);
                    text_buf.clear();
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_string();
                if name == "ConversionProfile" {
                    parsed.has_conversion_profile = true;
                } else if name == "PropertyGroup" && !group_done {
                    parsed.has_property_group = true;
                    group_done = true;
                    for attr in e.attributes().with_checks(false).flatten() {
                        let k = attr.key.as_ref();
                        if let Ok(v) = attr.normalized_value(XmlVersion::Implicit1_0) {
                            match k {
                                "device" => parsed.device = v.into_owned(),
                                "label" => parsed.label = v.into_owned(),
                                _ => {}
                            }
                        }
                    }
                } else if in_first_group {
                    parsed.fields.entry(name).or_default();
                }
            }
            Ok(Event::Text(e)) => {
                if current_field.is_some() {
                    text_buf.push_str(&e.xml_content(XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::CData(e)) => {
                if current_field.is_some() {
                    text_buf.push_str(&e.xml_content(XmlVersion::Implicit1_0));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if current_field.is_some() {
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
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                if qname.as_ref() == "PropertyGroup" && in_first_group {
                    in_first_group = false;
                    group_done = true;
                    current_field = None;
                } else if let Some(field) = current_field.take() {
                    parsed.fields.entry(field).or_insert_with(|| text_buf.trim().to_string());
                    text_buf.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Invalid XML in conversion profile: {e}")),
            _ => {}
        }
    }
    Ok(parsed)
}

/// Parses an FP1/FP2/FP3 profile.
pub fn parse(xml: &str) -> Result<Profile, String> {
    if xml.len() > MAX_SIZE {
        return Err("Profile is too large".to_string());
    }
    let parsed = parse_fp1_xml(xml)?;
    if !parsed.has_conversion_profile || !parsed.has_property_group {
        return Err("Not an X RAW Studio conversion profile".to_string());
    }
    let field = |name: &str| parsed.fields.get(name).map(String::as_str).unwrap_or("");

    let sim_name = field("FilmSimulation");
    let film_simulation = FILM_SIMS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(sim_name))
        .map(|&(_, s)| s)
        .ok_or_else(|| format!("Unknown film simulation \"{}\"", sim_name))?;

    let wb = field("WhiteBalance");
    let white_balance = WHITE_BALANCES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(wb))
        .map(|&(_, w)| w)
        // "AsShot" and "INVALID" keep the photo's own white balance
        .unwrap_or("As Shot")
        .to_string();
    let color_temperature = (white_balance == "Kelvin")
        .then(|| field("WBColorTemp").trim_end_matches('K').parse::<u32>().ok())
        .flatten()
        .filter(|&k| k > 0);

    let dr = field("DynamicRange").trim_start_matches("DR");
    let dynamic_range_auto = dr.eq_ignore_ascii_case("auto") || dr == "0";
    let dynamic_range = dr.parse::<u32>().ok().filter(|v| [100, 200, 400].contains(v));

    let device = parsed.device;
    let recipe = CameraRecipe {
        camera: device.clone(),
        film_simulation,
        film_simulation_name: film_simulation.display_name().to_string(),
        grain: strength(field("GrainEffect")),
        grain_size: if field("GrainEffectSize").eq_ignore_ascii_case("LARGE") {
            GrainSize::Large
        } else {
            GrainSize::Small
        },
        color_chrome: strength(field("ChromeEffect")),
        color_chrome_fx_blue: strength(field("ColorChromeBlue")),
        white_balance,
        color_temperature,
        wb_shift: (int_clamped(field("WBShiftR"), -9, 9), int_clamped(field("WBShiftB"), -9, 9)),
        dynamic_range,
        dynamic_range_auto,
        d_range_priority: None,
        highlight: tone_clamped(field("HighlightTone")),
        shadow: tone_clamped(field("ShadowTone")),
        color: int_clamped(field("Color"), -5, 5),
        sharpness: int_clamped(field("Sharpness"), -5, 5),
        noise_reduction: int_clamped(field("NoisReduction"), -5, 5),
        clarity: int_clamped(field("Clarity"), -5, 5),
        mono_warm_cool: int_clamped(field("BlackImageTone"), -18, 18),
        mono_magenta_green: int_clamped(field("MonochromaticColor_RG"), -18, 18),
        exposure_bias: parse_ev(field("ExposureBias")),
    };
    Ok(Profile { label: parsed.label, device, recipe })
}

/// Writes a profile in the layout X RAW Studio saves.
pub fn write(profile: &Profile) -> String {
    let r = &profile.recipe;
    let sim = FILM_SIMS.iter().find(|(_, s)| *s == r.film_simulation).map(|(n, _)| *n).unwrap_or("Provia");
    let wb = WHITE_BALANCES.iter().find(|(_, w)| *w == r.white_balance).map(|(n, _)| *n).unwrap_or("AsShot");
    let kelvin = r.color_temperature.map(|k| format!("{}K", k)).unwrap_or_else(|| "0K".to_string());
    let dr = if r.dynamic_range_auto { "0".to_string() } else { r.dynamic_range.unwrap_or(100).to_string() };
    let half = |v: f32| {
        if !v.is_finite() {
            return "0".to_string();
        }
        let v = (v.clamp(-2.0, 4.0) * 2.0).round() / 2.0;
        if v.fract() == 0.0 { format!("{}", v as i32) } else { format!("{:.1}", v) }
    };
    let device = escape_xml(&profile.device);
    let fields: Vec<(&str, String)> = vec![
        ("SerialNumber", String::new()),
        ("TetherRAWConditonCode", String::new()),
        ("Editable", "TRUE".into()),
        ("SourceFileName", String::new()),
        ("Fileerror", "NONE".into()),
        ("RotationAngle", "0".into()),
        ("StructVer", "65536".into()),
        ("IOPCode", String::new()),
        ("ShootingCondition", "OFF".into()),
        ("FileType", "JPG".into()),
        ("ImageSize", "L3x2".into()),
        ("ImageQuality", "Fine".into()),
        ("ExposureBias", format_ev(r.exposure_bias)),
        ("DynamicRange", dr),
        ("WideDRange", "0".into()),
        ("FilmSimulation", sim.into()),
        ("BlackImageTone", r.mono_warm_cool.to_string()),
        ("MonochromaticColor_RG", r.mono_magenta_green.to_string()),
        ("GrainEffect", strength_name(r.grain).into()),
        ("GrainEffectSize", if r.grain_size == GrainSize::Large { "LARGE" } else { "SMALL" }.into()),
        ("ChromeEffect", strength_name(r.color_chrome).into()),
        ("ColorChromeBlue", strength_name(r.color_chrome_fx_blue).into()),
        ("SmoothSkinEffect", "OFF".into()),
        ("WBShootCond", "OFF".into()),
        ("WhiteBalance", wb.into()),
        ("WBShiftR", r.wb_shift.0.to_string()),
        ("WBShiftB", r.wb_shift.1.to_string()),
        ("WBColorTemp", kelvin),
        ("HighlightTone", half(r.highlight)),
        ("ShadowTone", half(r.shadow)),
        ("Color", r.color.to_string()),
        ("Sharpness", r.sharpness.to_string()),
        ("NoisReduction", r.noise_reduction.to_string()),
        ("Clarity", r.clarity.to_string()),
        ("LensModulationOpt", "ON".into()),
        ("ColorSpace", "sRGB".into()),
        ("HDR", String::new()),
    ];
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ConversionProfile application=\"XRFC\" version=\"1.10.0.0\">\n");
    out += &format!(
        "    <PropertyGroup device=\"{}\" version=\"{}_0100\" label=\"{}\">\n",
        device,
        device,
        escape_xml(&profile.label)
    );
    for (name, value) in fields {
        if value.is_empty() {
            out += &format!("        <{}/>\n", name);
        } else {
            out += &format!("        <{0}>{1}</{0}>\n", name, escape_xml(&value));
        }
    }
    out += "    </PropertyGroup>\n</ConversionProfile>\n";
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORTRA: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ConversionProfile application="XRFC" version="1.10.0.0">
    <PropertyGroup device="X-T3" version="X-T3_0100" label="Kodak Portra 400">
        <ExposureBias>P0P67</ExposureBias>
        <DynamicRange>400</DynamicRange>
        <FilmSimulation>Classic</FilmSimulation>
        <BlackImageTone>0</BlackImageTone>
        <GrainEffect>STRONG</GrainEffect>
        <GrainEffectSize>SMALL</GrainEffectSize>
        <ChromeEffect>STRONG</ChromeEffect>
        <ColorChromeBlue/>
        <WhiteBalance>Daylight</WhiteBalance>
        <WBShiftR>3</WBShiftR>
        <WBShiftB>-5</WBShiftB>
        <WBColorTemp>10000K</WBColorTemp>
        <HighlightTone>-1</HighlightTone>
        <ShadowTone>-2</ShadowTone>
        <Color>2</Color>
        <Sharpness>-2</Sharpness>
        <NoisReduction>-4</NoisReduction>
        <Clarity>0</Clarity>
    </PropertyGroup>
</ConversionProfile>"#;

    #[test]
    fn reads_an_x_raw_studio_profile() {
        let p = parse(PORTRA).unwrap();
        assert_eq!((p.label.as_str(), p.device.as_str()), ("Kodak Portra 400", "X-T3"));
        let r = &p.recipe;
        assert_eq!(r.film_simulation, FilmSim::ClassicChrome);
        assert_eq!((r.grain, r.color_chrome, r.color_chrome_fx_blue), (Strength::Strong, Strength::Strong, Strength::Off));
        assert_eq!((r.white_balance.as_str(), r.wb_shift, r.color_temperature), ("Daylight", (3, -5), None));
        assert_eq!((r.highlight, r.shadow, r.color, r.sharpness, r.noise_reduction), (-1.0, -2.0, 2, -2, -4));
        assert_eq!(r.dynamic_range, Some(400));
        assert!((r.exposure_bias - 2.0 / 3.0).abs() < 1e-5);
    }

    #[test]
    fn written_profiles_read_back_the_same() {
        let mut p = parse(PORTRA).unwrap();
        p.label = "Mine & \"yours\"".to_string();
        p.recipe.highlight = 1.5;
        p.recipe.white_balance = "Kelvin".to_string();
        p.recipe.color_temperature = Some(5600);
        let back = parse(&write(&p)).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn exposure_steps() {
        assert_eq!(format_ev(-1.0 / 3.0), "M0P33");
        assert_eq!(format_ev(1.67), "P1P67");
        assert_eq!(parse_ev("M2P33"), -7.0 / 3.0);
        assert_eq!(parse_ev("0"), 0.0);
        assert_eq!(format_ev(f32::NEG_INFINITY), "0");
        assert_eq!(format_ev(f32::NAN), "0");
        assert_eq!(format_ev(-99.0), "M5P00");
        assert_eq!(parse_ev("NaN"), 0.0);
        assert_eq!(parse_ev("M99P00"), -5.0);
    }

    #[test]
    fn supports_extended_wb_modes_and_clamps_out_of_range_fields() {
        let xml = PORTRA
            .replace("<WhiteBalance>Daylight</WhiteBalance>", "<WhiteBalance>AutoWhitePriority</WhiteBalance>")
            .replace("<HighlightTone>-1</HighlightTone>", "<HighlightTone>NaN</HighlightTone>")
            .replace("<WBShiftR>3</WBShiftR>", "<WBShiftR>999</WBShiftR>");
        let p = parse(&xml).unwrap();
        assert_eq!(p.recipe.white_balance, "Auto (white priority)");
        assert_eq!(p.recipe.highlight, 0.0);
        assert_eq!(p.recipe.wb_shift.0, 9);

        let custom = PORTRA.replace("<WhiteBalance>Daylight</WhiteBalance>", "<WhiteBalance>Custom2</WhiteBalance>");
        assert_eq!(parse(&custom).unwrap().recipe.white_balance, "Custom");
    }

    #[test]
    fn rejects_other_xml() {
        assert!(parse("<html></html>").is_err());
        assert!(parse(&PORTRA.replace("Classic", "Kodachrome")).is_err());
    }

    #[test]
    fn handles_comments_single_quotes_cdata_and_numeric_entities() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ConversionProfile application="XRFC">
    <!-- <PropertyGroup device="WRONG" label="commented"/> -->
    <PropertyGroup device='GFX 100S' label='Warm &#38; &#x22;Soft&#x22;'>
        <!-- <FilmSimulation>Velvia</FilmSimulation> -->
        <FilmSimulation><![CDATA[ClassicNEGA]]></FilmSimulation>
        <WhiteBalance>Temperature</WhiteBalance>
        <WBColorTemp>5400K</WBColorTemp>
        <DynamicRange>DR200</DynamicRange>
    </PropertyGroup>
</ConversionProfile>"#;
        let p = parse(xml).unwrap();
        assert_eq!(p.device, "GFX 100S");
        assert_eq!(p.label, "Warm & \"Soft\"");
        assert_eq!(p.recipe.film_simulation, FilmSim::ClassicNeg);
        assert_eq!(p.recipe.color_temperature, Some(5400));
        assert_eq!(p.recipe.dynamic_range, Some(200));
    }
}
