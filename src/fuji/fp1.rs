//! X RAW Studio conversion profiles (.FP1, .FP2, .FP3).
//!
//! A profile is a flat XML file: `<ConversionProfile>` holding one `<PropertyGroup
//! device=".." label="..">` whose child elements are the camera menu settings by name
//! (`<FilmSimulation>Classic</FilmSimulation>`, `<GrainEffect>STRONG</GrainEffect>`...).
//! Value spellings follow profiles saved by X RAW Studio 1.x.

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
    ("Daylight", "Daylight"),
    ("Shade", "Shade"),
    ("FLight1", "Fluorescent"),
    ("FLight2", "Fluorescent"),
    ("FLight3", "Fluorescent"),
    ("Incandescent", "Incandescent"),
    ("UnderWater", "Underwater"),
    ("Temperature", "Kelvin"),
];

/// Text of the first `<name>...</name>` element; "" for `<name/>` or a missing one.
fn element<'a>(xml: &'a str, name: &str) -> &'a str {
    let open = format!("<{}>", name);
    let Some(start) = xml.find(&open).map(|i| i + open.len()) else { return "" };
    let close = format!("</{}>", name);
    xml[start..].find(&close).map(|end| xml[start..start + end].trim()).unwrap_or("")
}

/// Value of `attr="..."` inside the first `<tag ...>` start tag.
fn attribute(xml: &str, tag: &str, attr: &str) -> String {
    let Some(start) = xml.find(&format!("<{} ", tag)) else { return String::new() };
    let head = &xml[start..start + xml[start..].find('>').unwrap_or(0)];
    let key = format!("{}=\"", attr);
    head.find(&key)
        .and_then(|i| head[i + key.len()..].split('"').next())
        .map(unescape)
        .unwrap_or_default()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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
        _ => return v.parse().unwrap_or(0.0),
    };
    let value: f32 = rest.replacen('P', ".", 1).parse().unwrap_or(0.0);
    // .33/.67 are thirds of a stop
    sign * ((value * 3.0).round() / 3.0)
}

fn format_ev(ev: f32) -> String {
    let thirds = (ev * 3.0).round() as i32;
    if thirds == 0 {
        return "0".to_string();
    }
    let sign = if thirds > 0 { 'P' } else { 'M' };
    let (whole, frac) = (thirds.abs() / 3, thirds.abs() % 3);
    let frac = ["00", "33", "67"][frac as usize];
    format!("{}{}P{}", sign, whole, frac)
}

fn int(v: &str) -> i32 {
    v.trim_start_matches('+').parse::<f32>().map(|f| f as i32).unwrap_or(0)
}

/// Parses an FP1/FP2/FP3 profile.
pub fn parse(xml: &str) -> Result<Profile, String> {
    if xml.len() > MAX_SIZE {
        return Err("Profile is too large".to_string());
    }
    if !xml.contains("<ConversionProfile") || !xml.contains("<PropertyGroup") {
        return Err("Not an X RAW Studio conversion profile".to_string());
    }
    let sim_name = element(xml, "FilmSimulation");
    let film_simulation = FILM_SIMS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(sim_name))
        .map(|&(_, s)| s)
        .ok_or_else(|| format!("Unknown film simulation \"{}\"", sim_name))?;

    let wb = element(xml, "WhiteBalance");
    let white_balance = WHITE_BALANCES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(wb))
        .map(|&(_, w)| w)
        // "AsShot" and "INVALID" keep the photo's own white balance
        .unwrap_or("As Shot")
        .to_string();
    let color_temperature = (white_balance == "Kelvin")
        .then(|| element(xml, "WBColorTemp").trim_end_matches('K').parse::<u32>().ok())
        .flatten()
        .filter(|&k| k > 0);

    let dr = element(xml, "DynamicRange").trim_start_matches("DR");
    let dynamic_range_auto = dr.eq_ignore_ascii_case("auto") || dr == "0";
    let dynamic_range = dr.parse::<u32>().ok().filter(|v| [100, 200, 400].contains(v));

    let device = attribute(xml, "PropertyGroup", "device");
    let recipe = CameraRecipe {
        camera: device.clone(),
        film_simulation,
        film_simulation_name: film_simulation.display_name().to_string(),
        grain: strength(element(xml, "GrainEffect")),
        grain_size: if element(xml, "GrainEffectSize").eq_ignore_ascii_case("LARGE") {
            GrainSize::Large
        } else {
            GrainSize::Small
        },
        color_chrome: strength(element(xml, "ChromeEffect")),
        color_chrome_fx_blue: strength(element(xml, "ColorChromeBlue")),
        white_balance,
        color_temperature,
        wb_shift: (int(element(xml, "WBShiftR")), int(element(xml, "WBShiftB"))),
        dynamic_range,
        dynamic_range_auto,
        d_range_priority: None,
        highlight: element(xml, "HighlightTone").parse().unwrap_or(0.0),
        shadow: element(xml, "ShadowTone").parse().unwrap_or(0.0),
        color: int(element(xml, "Color")),
        sharpness: int(element(xml, "Sharpness")),
        noise_reduction: int(element(xml, "NoisReduction")),
        clarity: int(element(xml, "Clarity")),
        mono_warm_cool: int(element(xml, "BlackImageTone")),
        mono_magenta_green: int(element(xml, "MonochromaticColor_RG")),
        exposure_bias: parse_ev(element(xml, "ExposureBias")),
    };
    Ok(Profile { label: attribute(xml, "PropertyGroup", "label"), device, recipe })
}

/// Writes a profile in the layout X RAW Studio saves.
pub fn write(profile: &Profile) -> String {
    let r = &profile.recipe;
    let sim = FILM_SIMS.iter().find(|(_, s)| *s == r.film_simulation).map(|(n, _)| *n).unwrap_or("Provia");
    let wb = WHITE_BALANCES.iter().find(|(_, w)| *w == r.white_balance).map(|(n, _)| *n).unwrap_or("AsShot");
    let kelvin = r.color_temperature.map(|k| format!("{}K", k)).unwrap_or_else(|| "0K".to_string());
    let dr = if r.dynamic_range_auto { "0".to_string() } else { r.dynamic_range.unwrap_or(100).to_string() };
    let half = |v: f32| {
        let v = (v * 2.0).round() / 2.0;
        if v.fract() == 0.0 { format!("{}", v as i32) } else { format!("{:.1}", v) }
    };
    let device = escape(&profile.device);
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
        escape(&profile.label)
    );
    for (name, value) in fields {
        if value.is_empty() {
            out += &format!("        <{}/>\n", name);
        } else {
            out += &format!("        <{0}>{1}</{0}>\n", name, value);
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
    }

    #[test]
    fn rejects_other_xml() {
        assert!(parse("<html></html>").is_err());
        assert!(parse(&PORTRA.replace("Classic", "Kodachrome")).is_err());
    }
}
