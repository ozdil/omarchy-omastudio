//! Recipes written as text, the way recipe sites and forums share them:
//!
//! ```text
//! Classic Chrome
//! Dynamic Range: DR400
//! Highlight: -1
//! Shadow: +1.5
//! Color: +2
//! Grain Effect: Strong, Small
//! Color Chrome Effect: Strong
//! Color Chrome FX Blue: Off
//! White Balance: Daylight, +3 Red & -5 Blue
//! Exposure Compensation: +1/3 to +2/3 (typically)
//! ```
//!
//! Labels are matched loosely (case, spacing and punctuation are ignored) and
//! unrecognised lines are skipped, so pasted text with a title or notes still works.

use super::{CameraRecipe, FilmSim, GrainSize, Strength};

/// Lowercase letters and digits only.
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

fn film_sim(value: &str) -> Option<FilmSim> {
    let v = norm(value);
    let v = v.strip_suffix("filter").unwrap_or(&v);
    Some(match v {
        "provia" | "proviastandard" | "standard" => FilmSim::Provia,
        "velvia" | "velviavivid" | "vivid" => FilmSim::Velvia,
        "astia" | "astiasoft" | "soft" => FilmSim::Astia,
        "classicchrome" => FilmSim::ClassicChrome,
        "realaace" => FilmSim::RealaAce,
        "pronegstd" | "pronegativestd" | "pronegstandard" => FilmSim::ProNegStd,
        "proneghi" | "pronegativehi" => FilmSim::ProNegHi,
        "classicneg" | "classicnegative" => FilmSim::ClassicNeg,
        "nostalgicneg" | "nostalgicnegative" => FilmSim::NostalgicNeg,
        "eterna" | "eternacinema" | "cinema" => FilmSim::Eterna,
        "eternableachbypass" | "bleachbypass" => FilmSim::BleachBypass,
        "acros" => FilmSim::Acros,
        "acrosye" | "acrosyellow" | "acrosy" => FilmSim::AcrosYellow,
        "acrosr" | "acrosred" => FilmSim::AcrosRed,
        "acrosg" | "acrosgreen" => FilmSim::AcrosGreen,
        "monochrome" | "mono" | "bw" => FilmSim::Monochrome,
        "monochromeye" | "monochromeyellow" | "monoye" | "monoyellow" => FilmSim::MonochromeYellow,
        "monochromer" | "monochromered" | "monor" | "monored" => FilmSim::MonochromeRed,
        "monochromeg" | "monochromegreen" | "monog" | "monogreen" => FilmSim::MonochromeGreen,
        "sepia" => FilmSim::Sepia,
        _ => return None,
    })
}

/// Off/Weak/Strong anywhere in the value.
fn strength(value: &str) -> Option<Strength> {
    let v = value.to_ascii_lowercase();
    if v.contains("strong") {
        Some(Strength::Strong)
    } else if v.contains("weak") {
        Some(Strength::Weak)
    } else if v.contains("off") {
        Some(Strength::Off)
    } else {
        None
    }
}

/// First signed number in the value: "+1.5", "-2", "0 (normal)".
/// Skips bare `+` or `-` punctuation that is not immediately followed by a digit.
fn number(value: &str) -> Option<f32> {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let is_start = b.is_ascii_digit()
            || ((b == b'+' || b == b'-') && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit());
        if is_start {
            let tail = &value[i..];
            let end = tail
                .char_indices()
                .skip(1)
                .find(|&(_, c)| !(c.is_ascii_digit() || c == '.'))
                .map(|(j, _)| j)
                .unwrap_or(tail.len());
            if let Ok(parsed) = tail[..end].trim_start_matches('+').parse::<f32>()
                && parsed.is_finite()
            {
                return Some(parsed);
            }
            i += end.max(1);
        } else {
            i += 1;
        }
    }
    None
}

/// "+1/3", "-2/3", "+1", "+1 1/3", "+1/3 to +2/3" (takes the first).
fn exposure(value: &str) -> Option<f32> {
    // Split on word-bounded " to " (case-insensitive) so words like "stop" are never split.
    let lower = value.to_ascii_lowercase();
    let first = match lower.find(" to ") {
        Some(idx) => value[..idx].trim(),
        None => value.trim(),
    };
    let sign = if first.starts_with('-') { -1.0 } else { 1.0 };
    let digits = first.trim_start_matches(['+', '-']);
    let mut total = 0.0;
    let mut found = false;
    for part in digits.split_whitespace() {
        if let Some((n, d)) = part.split_once('/') {
            let (Ok(n), Ok(d)) = (n.parse::<f32>(), d.trim_end_matches(|c: char| !c.is_ascii_digit()).parse::<f32>()) else {
                break;
            };
            if d > 0.0 && n.is_finite() && d.is_finite() {
                total += n / d;
                found = true;
            }
            break;
        }
        match part.parse::<f32>() {
            Ok(v) if v.is_finite() => {
                total += v;
                found = true;
            }
            _ => break,
        }
    }
    found.then_some((sign * total).clamp(-5.0, 5.0))
}

fn white_balance(value: &str, r: &mut CameraRecipe) {
    let lower = value.to_ascii_lowercase();
    if let Some(k) = lower.split(|c: char| !c.is_ascii_alphanumeric()).find_map(|w| {
        w.strip_suffix('k').and_then(|n| n.parse::<u32>().ok()).filter(|k| (2000..=12000).contains(k))
    }) {
        r.white_balance = "Kelvin".to_string();
        r.color_temperature = Some(k);
    } else {
        let mode = norm(value.split(',').next().unwrap_or(""));
        r.white_balance = match mode.as_str() {
            m if m.starts_with("auto") => "Auto",
            "daylight" | "sunny" | "fineweather" => "Daylight",
            "shade" | "cloudy" => "Shade",
            m if m.starts_with("fluorescent") => "Fluorescent",
            "incandescent" | "tungsten" => "Incandescent",
            "underwater" => "Underwater",
            _ => "As Shot",
        }
        .to_string();
    }
    // "+3 Red & -5 Blue"
    let words: Vec<&str> = value.split(|c: char| c.is_whitespace() || c == '&' || c == ',').filter(|w| !w.is_empty()).collect();
    for pair in words.windows(2) {
        let Some(n) = number(pair[0]).filter(|_| pair[0].chars().any(|c| c.is_ascii_digit())) else { continue };
        match norm(pair[1]).as_str() {
            "red" | "r" => r.wb_shift.0 = n as i32,
            "blue" | "b" => r.wb_shift.1 = n as i32,
            _ => {}
        }
    }
}

/// Parses a text recipe. Fails only when nothing in it is recognised.
pub fn parse(text: &str) -> Result<CameraRecipe, String> {
    let mut r = CameraRecipe { white_balance: "As Shot".to_string(), ..CameraRecipe::default() };
    let mut recognised = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()).take(200) {
        let Some((label, value)) = line.split_once(':') else {
            // A bare film simulation name, often the first line
            if let Some(sim) = film_sim(line) {
                r.film_simulation = sim;
                recognised += 1;
            }
            continue;
        };
        let value = value.trim();
        let label = norm(label);
        let ok = match label.as_str() {
            "filmsimulation" | "film" | "simulation" | "filmsim" => film_sim(value).map(|s| r.film_simulation = s),
            "dynamicrange" | "dr" => {
                let v = norm(value);
                if v.starts_with("auto") {
                    r.dynamic_range_auto = true;
                    Some(())
                } else {
                    number(&value.to_ascii_lowercase().replace("dr", ""))
                        .filter(|&n| (100.0..=800.0).contains(&n))
                        .map(|n| r.dynamic_range = Some(n.round() as u32))
                }
            }
            "drangepriority" => {
                r.d_range_priority = Some(value.to_string()).filter(|v| !v.eq_ignore_ascii_case("off"));
                Some(())
            }
            "highlight" | "highlights" | "highlighttone" => number(value).map(|n| r.highlight = n),
            "shadow" | "shadows" | "shadowtone" => number(value).map(|n| r.shadow = n),
            "color" | "colour" | "saturation" => number(value).map(|n| r.color = n as i32),
            "sharpness" | "sharpening" => number(value).map(|n| r.sharpness = n as i32),
            "highisonr" | "noisereduction" | "nr" | "highisonoisereduction" => {
                number(value).map(|n| r.noise_reduction = n as i32)
            }
            "clarity" => number(value).map(|n| r.clarity = n as i32),
            "graineffect" | "grain" => strength(value).map(|s| {
                r.grain = s;
                r.grain_size = if value.to_ascii_lowercase().contains("large") { GrainSize::Large } else { GrainSize::Small };
            }),
            "colorchromeeffect" | "colorchrome" | "colourchromeeffect" => strength(value).map(|s| r.color_chrome = s),
            "colorchromefxblue" | "colorchromeeffectblue" | "fxblue" | "colourchromefxblue" => {
                strength(value).map(|s| r.color_chrome_fx_blue = s)
            }
            "whitebalance" | "wb" => {
                white_balance(value, &mut r);
                Some(())
            }
            "monochromaticcolor" | "monochromaticcolour" | "toning" => {
                // "WC +2 & MG -1"
                let v = value.to_ascii_lowercase();
                let after = |key: &str| v.find(key).and_then(|i| number(&v[i + key.len()..]));
                if let Some(n) = after("wc") {
                    r.mono_warm_cool = n as i32;
                }
                if let Some(n) = after("mg") {
                    r.mono_magenta_green = n as i32;
                }
                Some(())
            }
            "exposurecompensation" | "exposure" | "ev" => exposure(value).map(|e| r.exposure_bias = e),
            _ => None,
        };
        if ok.is_some() {
            recognised += 1;
        }
    }
    if recognised == 0 {
        return Err("No recipe settings found in the text".to_string());
    }
    r.film_simulation_name = r.film_simulation.display_name().to_string();
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_shared_recipe() {
        let r = parse(
            "My Portra\nFilm Simulation: Classic Negative\nDynamic Range: DR400\nGrain Effect: Strong, Large\n\
             Color Chrome Effect: Weak\nColor Chrome FX Blue: Strong\n\
             White Balance: Daylight, +3 Red & -5 Blue\nHighlight: -1\nShadow: +1.5\nColor: +2\n\
             Sharpness: -2\nHigh ISO NR: -4\nClarity: -3\nISO: Auto, up to ISO 6400\n\
             Exposure Compensation: +1/3 to +2/3 (typically)",
        )
        .unwrap();
        assert_eq!(r.film_simulation, FilmSim::ClassicNeg);
        assert_eq!(r.dynamic_range, Some(400));
        assert_eq!((r.grain, r.grain_size), (Strength::Strong, GrainSize::Large));
        assert_eq!((r.color_chrome, r.color_chrome_fx_blue), (Strength::Weak, Strength::Strong));
        assert_eq!((r.white_balance.as_str(), r.wb_shift), ("Daylight", (3, -5)));
        assert_eq!((r.highlight, r.shadow, r.color, r.sharpness, r.noise_reduction, r.clarity), (-1.0, 1.5, 2, -2, -4, -3));
        assert!((r.exposure_bias - 1.0 / 3.0).abs() < 1e-5);
    }

    #[test]
    fn kelvin_and_bare_simulation_lines() {
        let r = parse("Acros+R\nWhite Balance: 5600K, -2 Red & +1 Blue").unwrap();
        assert_eq!(r.film_simulation, FilmSim::AcrosRed);
        assert_eq!((r.white_balance.as_str(), r.color_temperature, r.wb_shift), ("Kelvin", Some(5600), (-2, 1)));
    }

    #[test]
    fn unrelated_text_is_refused() {
        assert!(parse("hello world\nnothing here").is_err());
    }

    #[test]
    fn handles_bare_dashes_stop_word_and_negative_dr() {
        let r = parse(
            "Classic Chrome\nColor: Fuji - vivid +2\nExposure: +1 stop\nDynamic Range: -100",
        )
        .unwrap();
        assert_eq!(r.color, 2);
        assert!((r.exposure_bias - 1.0).abs() < 1e-5);
        // Negative DR is rejected; default 100 stays intact instead of wrapping to u32::MAX
        assert_eq!(r.dynamic_range, Some(100));
    }
}
