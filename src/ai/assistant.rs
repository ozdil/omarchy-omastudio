//! Edit assistant: a vision model looks at the photo as currently developed and
//! returns new values for the global adjustments, following the user's request.
//! The answer is constrained by a JSON schema, then clamped to each field's range.

use serde_json::{json, Map, Value};

use crate::recipe::Recipe;

/// Fields the model sets, with their ranges.
const NUMERIC: &[(&str, f64, f64, &str)] = &[
    ("exposure", -5.0, 5.0, "EV"),
    ("wb_temperature", 2000.0, 12000.0, "Kelvin, 5500 = as shot; higher is warmer"),
    ("wb_tint", -100.0, 100.0, "negative green, positive magenta"),
    ("contrast", -100.0, 100.0, ""),
    ("highlights", -100.0, 100.0, "negative recovers highlights"),
    ("shadows", -100.0, 100.0, "positive lifts shadows"),
    ("whites", -100.0, 100.0, ""),
    ("blacks", -100.0, 100.0, ""),
    ("texture", -100.0, 100.0, ""),
    ("clarity", -100.0, 100.0, ""),
    ("dehaze", -100.0, 100.0, ""),
    ("vibrance", -100.0, 100.0, ""),
    ("saturation", -100.0, 100.0, ""),
    ("vignette", -100.0, 100.0, "negative darkens the corners"),
    ("grain_amount", 0.0, 100.0, "film grain"),
];

pub const FILM_SIMULATIONS: &[&str] = &[
    "none", "fuji_provia", "fuji_velvia", "fuji_astia", "fuji_classic_chrome", "fuji_reala_ace", "fuji_pro_neg_hi",
    "fuji_pro_neg_std", "fuji_classic_neg", "fuji_nostalgic_neg", "fuji_eterna", "fuji_eterna_bleach_bypass",
    "fuji_acros_standard", "fuji_acros_yellow", "fuji_acros_red", "fuji_acros_green", "fuji_monochrome",
    "fuji_monochrome_yellow", "fuji_monochrome_red", "fuji_monochrome_green", "fuji_sepia",
];

pub const SYSTEM: &str = "You are the edit assistant of OmaStudio, a RAW photo editor. You see the photo \
as it is currently developed, its current adjustment values and the camera data. Choose new values for \
the adjustments that best fulfil the user's request; without a request, make a natural, balanced edit \
that a careful photographer would make. Keep values you have no reason to change. Prefer moderate \
changes, protect highlights and skin tones, and keep the white balance believable. Explain the edit \
in one or two short sentences in the language of the user's request (English when there is none).";

/// The JSON schema of the answer: every field is required, so strict modes accept it.
pub fn schema() -> Value {
    let mut props = Map::new();
    props.insert("explanation".into(), json!({ "type": "string" }));
    for (name, min, max, note) in NUMERIC {
        // Ranges go in the description: strict schema modes reject minimum/maximum, and
        // merge() clamps the answer anyway
        let p = json!({ "type": "number", "description": format!("{min} to {max} {note}").trim().to_string() });
        props.insert((*name).into(), p);
    }
    props.insert("film_simulation".into(), json!({ "type": "string", "enum": FILM_SIMULATIONS }));
    let required: Vec<&str> = std::iter::once("explanation")
        .chain(NUMERIC.iter().map(|f| f.0))
        .chain(std::iter::once("film_simulation"))
        .collect();
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

/// The text part of the request: the user's wish, current values and camera data.
pub fn request_text(prompt: &str, current: &Recipe, camera: &Value) -> String {
    let cur = serde_json::to_value(current).unwrap_or_default();
    let mut values = Map::new();
    for (name, ..) in NUMERIC {
        values.insert((*name).into(), cur.get(*name).cloned().unwrap_or(json!(0)));
    }
    let sim = cur.get("film_simulation").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("none");
    values.insert("film_simulation".into(), json!(sim));
    let wish = if prompt.trim().is_empty() { "No specific request: make a natural, balanced edit." } else { prompt.trim() };
    format!(
        "Request: {wish}\n\nCurrent values: {}\n\nCamera data: {}",
        Value::Object(values),
        camera
    )
}

/// Applies the model's answer to the current recipe; returns the recipe and explanation.
pub fn merge(current: &Recipe, answer: &Value) -> (Recipe, String) {
    let mut recipe = serde_json::to_value(current).unwrap_or_else(|_| json!({}));
    for (name, min, max, _) in NUMERIC {
        if let Some(v) = answer.get(*name).and_then(Value::as_f64).filter(|v| v.is_finite()) {
            recipe[*name] = json!(v.clamp(*min, *max));
        }
    }
    if let Some(sim) = answer.get("film_simulation").and_then(Value::as_str).filter(|s| FILM_SIMULATIONS.contains(s)) {
        recipe["film_simulation"] = json!(sim);
    }
    let explanation = answer.get("explanation").and_then(Value::as_str).unwrap_or("").chars().take(600).collect();
    (serde_json::from_value(recipe).unwrap_or_else(|_| current.clone()), explanation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_clamps_and_ignores_unknown_values() {
        let current = Recipe { contrast: 10.0, ..Recipe::default() };
        let answer = json!({ "exposure": 9.0, "shadows": 30, "film_simulation": "kodak_gold", "explanation": "Brighter.", "sharpness": 100 });
        let (r, why) = merge(&current, &answer);
        assert_eq!(r.exposure, 5.0);
        assert_eq!(r.shadows, 30.0);
        assert_eq!(r.contrast, 10.0);
        assert_eq!(r.film_simulation, current.film_simulation);
        assert_eq!(r.sharpness, current.sharpness);
        assert_eq!(why, "Brighter.");
    }

    #[test]
    fn schema_requires_every_field() {
        let s = schema();
        assert_eq!(s["required"].as_array().unwrap().len(), s["properties"].as_object().unwrap().len());
    }
}
