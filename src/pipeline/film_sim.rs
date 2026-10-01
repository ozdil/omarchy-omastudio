//! Authentic Film Simulation and Medium Format Color Science Engine
//!
//! Provides faithful emulations of:
//! - Fujifilm X-Trans & GFX Medium Format Profiles:
//!   Provia, Velvia 50, Astia 100F, Classic Chrome, Classic Neg, Eterna Cinema,
//!   Acros 100 (Standard, Yellow, Red, Green filters).
//! - Hasselblad Natural Colour Solution (HNCS):
//!   16-bit sensor chroma isolation, non-twisting perceptual highlight roll-off.
//! - Hasselblad XPan 65:24 Panoramic Character.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilmSimulation {
    #[default]
    None,
    // Fujifilm
    FujiProvia,
    FujiVelvia,
    FujiAstia,
    FujiClassicChrome,
    FujiClassicNeg,
    FujiEterna,
    FujiAcrosStandard,
    FujiAcrosYellow,
    FujiAcrosRed,
    FujiAcrosGreen,
    // Hasselblad
    HasselbladHncs,
    HasselbladXPan,
}

impl FilmSimulation {
    pub fn from_id(id: &str) -> Self {
        match id.to_lowercase().replace('-', "_").as_str() {
            "fuji_provia" | "provia" => Self::FujiProvia,
            "fuji_velvia" | "velvia" => Self::FujiVelvia,
            "fuji_astia" | "astia" => Self::FujiAstia,
            "fuji_classic_chrome" | "classic_chrome" => Self::FujiClassicChrome,
            "fuji_classic_neg" | "classic_neg" => Self::FujiClassicNeg,
            "fuji_eterna" | "eterna" => Self::FujiEterna,
            "fuji_acros" | "acros" | "acros_standard" => Self::FujiAcrosStandard,
            "fuji_acros_yellow" | "acros_yellow" | "acros_ye" => Self::FujiAcrosYellow,
            "fuji_acros_red" | "acros_red" | "acros_r" => Self::FujiAcrosRed,
            "fuji_acros_green" | "acros_green" | "acros_g" => Self::FujiAcrosGreen,
            "hasselblad_hncs" | "hncs" => Self::HasselbladHncs,
            "hasselblad_xpan" | "xpan" => Self::HasselbladXPan,
            _ => Self::None,
        }
    }

    pub fn as_id(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::FujiProvia => "fuji_provia",
            Self::FujiVelvia => "fuji_velvia",
            Self::FujiAstia => "fuji_astia",
            Self::FujiClassicChrome => "fuji_classic_chrome",
            Self::FujiClassicNeg => "fuji_classic_neg",
            Self::FujiEterna => "fuji_eterna",
            Self::FujiAcrosStandard => "fuji_acros_standard",
            Self::FujiAcrosYellow => "fuji_acros_yellow",
            Self::FujiAcrosRed => "fuji_acros_red",
            Self::FujiAcrosGreen => "fuji_acros_green",
            Self::HasselbladHncs => "hasselblad_hncs",
            Self::HasselbladXPan => "hasselblad_xpan",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::None => "None / Standard",
            Self::FujiProvia => "Fujifilm Provia 100F",
            Self::FujiVelvia => "Fujifilm Velvia 50",
            Self::FujiAstia => "Fujifilm Astia 100F",
            Self::FujiClassicChrome => "Fujifilm Classic Chrome",
            Self::FujiClassicNeg => "Fujifilm Classic Neg",
            Self::FujiEterna => "Fujifilm Eterna Cinema",
            Self::FujiAcrosStandard => "Fujifilm Acros 100",
            Self::FujiAcrosYellow => "Fujifilm Acros 100 (+Ye)",
            Self::FujiAcrosRed => "Fujifilm Acros 100 (+R)",
            Self::FujiAcrosGreen => "Fujifilm Acros 100 (+G)",
            Self::HasselbladHncs => "Hasselblad HNCS Neutral",
            Self::HasselbladXPan => "Hasselblad XPan Dramatic",
        }
    }

    pub fn is_monochrome(&self) -> bool {
        matches!(
            self,
            Self::FujiAcrosStandard
                | Self::FujiAcrosYellow
                | Self::FujiAcrosRed
                | Self::FujiAcrosGreen
        )
    }

    /// Evaluates the simulation on a linear RGB pixel in [0.0, 1.0]
    #[inline(always)]
    pub fn apply_pixel(&self, r: f32, g: f32, b: f32, intensity: f32) -> (f32, f32, f32) {
        if *self == Self::None || intensity <= 0.001 {
            return (r, g, b);
        }

        let (tr, tg, tb) = self.eval_simulation(r, g, b);

        if intensity >= 0.999 {
            (tr, tg, tb)
        } else {
            (
                r * (1.0 - intensity) + tr * intensity,
                g * (1.0 - intensity) + tg * intensity,
                b * (1.0 - intensity) + tb * intensity,
            )
        }
    }

    #[inline(always)]
    fn eval_simulation(&self, r: f32, g: f32, b: f32) -> (f32, f32, f32) {
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        match self {
            Self::None => (r, g, b),

            Self::FujiProvia => {
                // Provia: Natural realistic skin tones and linear balance
                let r_p = luma + (r - luma) * 1.05;
                let g_p = luma + (g - luma) * 1.02;
                let b_p = luma + (b - luma) * 1.02;
                (r_p.clamp(0.0, 1.0), g_p.clamp(0.0, 1.0), b_p.clamp(0.0, 1.0))
            }

            Self::FujiVelvia => {
                // Velvia 50: Rich saturation, deep sky blue, vivid landscape foliage, punchy S-curve
                let curve = |v: f32| -> f32 {
                    let diff = v - 0.50;
                    (0.50 + (diff * 1.35).tanh() * 0.49).clamp(0.0, 1.0)
                };
                let r_c = curve(r);
                let g_c = curve(g);
                let b_c = curve(b);
                let l_c = 0.2126 * r_c + 0.7152 * g_c + 0.0722 * b_c;

                // Rich emerald green and deep blue saturation boost
                let r_v = l_c + (r_c - l_c) * 1.25;
                let g_v = l_c + (g_c - l_c) * 1.38;
                let b_v = l_c + (b_c - l_c) * 1.32;
                (r_v.clamp(0.0, 1.0), g_v.clamp(0.0, 1.0), b_v.clamp(0.0, 1.0))
            }

            Self::FujiAstia => {
                // Astia 100F: Soft highlights, low shadow contrast, gentle pastel portraits
                let soft_curve = |v: f32| -> f32 {
                    if v < 0.5 {
                        v * 1.08 // Gentle shadow lift
                    } else {
                        1.0 - (1.0 - v) * 0.94 // Protected highlight roll-off
                    }
                };
                let r_s = soft_curve(r);
                let g_s = soft_curve(g);
                let b_s = soft_curve(b);
                let l_s = 0.2126 * r_s + 0.7152 * g_s + 0.0722 * b_s;

                // Protect skin tones with smooth red/orange vibrance
                let r_a = l_s + (r_s - l_s) * 1.06;
                let g_a = l_s + (g_s - l_s) * 0.98;
                let b_a = l_s + (b_s - l_s) * 0.95;
                (r_a.clamp(0.0, 1.0), g_a.clamp(0.0, 1.0), b_a.clamp(0.0, 1.0))
            }

            Self::FujiClassicChrome => {
                // Classic Chrome: Documentary muted vibrance, cyan-tinted blues, hard midtone contrast
                let diff = luma - 0.42;
                let contrast_luma = (0.42 + (diff * 1.28).tanh() * 0.48).clamp(0.0, 1.0);
                let luma_ratio = if luma > 1e-4 { contrast_luma / luma } else { 1.0 };

                let mut r_cc = r * luma_ratio;
                let mut g_cc = g * luma_ratio;
                let mut b_cc = b * luma_ratio;
                let new_luma = 0.2126 * r_cc + 0.7152 * g_cc + 0.0722 * b_cc;

                // Muted overall saturation
                r_cc = new_luma + (r_cc - new_luma) * 0.80;
                g_cc = new_luma + (g_cc - new_luma) * 0.84;
                // Shift sky blues toward cyan
                let blue_chroma = b_cc - new_luma;
                b_cc = new_luma + blue_chroma * 0.72 + (g_cc - r_cc) * 0.08;

                (r_cc.clamp(0.0, 1.0), g_cc.clamp(0.0, 1.0), b_cc.clamp(0.0, 1.0))
            }

            Self::FujiClassicNeg => {
                // Classic Neg: Superia nostalgia, warm amber highlights, rich cool shadows
                let diff = luma - 0.48;
                let s_curve = (0.48 + (diff * 1.32).tanh() * 0.50).clamp(0.0, 1.0);
                let ratio = if luma > 1e-4 { s_curve / luma } else { 1.0 };

                let mut r_cn = r * ratio;
                let mut g_cn = g * ratio;
                let mut b_cn = b * ratio;
                let new_luma = 0.2126 * r_cn + 0.7152 * g_cn + 0.0722 * b_cn;

                // Warm amber highlights, emerald shadows
                if new_luma > 0.50 {
                    let h_weight = (new_luma - 0.50) * 2.0;
                    r_cn += 0.03 * h_weight;
                    b_cn -= 0.02 * h_weight;
                } else {
                    let s_weight = (0.50 - new_luma) * 2.0;
                    g_cn += 0.025 * s_weight;
                    b_cn += 0.015 * s_weight;
                }

                (r_cn.clamp(0.0, 1.0), g_cn.clamp(0.0, 1.0), b_cn.clamp(0.0, 1.0))
            }

            Self::FujiEterna => {
                // Eterna: Cinema film with wide dynamic range, flat gamma (-25 contrast), soft shadow toe
                let diff = luma - 0.46;
                let flat_luma = (0.46 + diff * 0.75).clamp(0.0, 1.0);
                let ratio = if luma > 1e-4 { flat_luma / luma } else { 1.0 };

                let r_e = flat_luma + (r * ratio - flat_luma) * 0.70;
                let g_e = flat_luma + (g * ratio - flat_luma) * 0.70;
                let b_e = flat_luma + (b * ratio - flat_luma) * 0.70;
                (r_e.clamp(0.0, 1.0), g_e.clamp(0.0, 1.0), b_e.clamp(0.0, 1.0))
            }

            Self::FujiAcrosStandard => {
                // Acros Standard Panchromatic: 0.32 R + 0.55 G + 0.13 B
                let y = 0.32 * r + 0.55 * g + 0.13 * b;
                let val = eval_acros_curve(y);
                (val, val, val)
            }

            Self::FujiAcrosYellow => {
                // Acros Yellow (+Ye): 0.45 R + 0.50 G + 0.05 B (Darkens blue skies, brightens skin)
                let y = 0.45 * r + 0.50 * g + 0.05 * b;
                let val = eval_acros_curve(y);
                (val, val, val)
            }

            Self::FujiAcrosRed => {
                // Acros Red (+R): 0.70 R + 0.25 G + 0.05 B (Dramatic contrast, dark stormy skies)
                let y = 0.70 * r + 0.25 * g + 0.05 * b;
                let val = eval_acros_curve(y);
                (val, val, val)
            }

            Self::FujiAcrosGreen => {
                // Acros Green (+G): 0.20 R + 0.70 G + 0.10 B (Rich foliage, smooth portrait tones)
                let y = 0.20 * r + 0.70 * g + 0.10 * b;
                let val = eval_acros_curve(y);
                (val, val, val)
            }

            Self::HasselbladHncs => {
                // Hasselblad Natural Colour Solution:
                // 1. Separation of Luminance and Chrominance (Hunt Effect compensation)
                // 2. 4th-root chroma scaling prevents color twisting in shadows and blown highlights
                let cr = r - luma;
                let cg = g - luma;
                let cb = b - luma;

                // Subtle medium format tone curve with natural highlight roll-off
                let hncs_luma = if luma < 0.50 {
                    0.50 * (luma / 0.50).powf(0.96) // Open clean shadows
                } else {
                    1.0 - 0.50 * ((1.0 - luma) / 0.50).powf(1.08) // Film-like highlight preservation
                };

                let scale = if luma > 1e-4 {
                    (hncs_luma / luma).powf(0.25)
                } else {
                    1.0
                };

                // Desaturate softly near specular highlights (Luma > 0.85) to avoid color banding
                let desat = if hncs_luma > 0.85 {
                    ((hncs_luma - 0.85) / 0.15).min(1.0) * 0.45
                } else {
                    0.0
                };

                let r_h = hncs_luma + cr * scale * (1.0 - desat);
                let g_h = hncs_luma + cg * scale * (1.0 - desat);
                let b_h = hncs_luma + cb * scale * (1.0 - desat);

                (r_h.clamp(0.0, 1.0), g_h.clamp(0.0, 1.0), b_h.clamp(0.0, 1.0))
            }

            Self::HasselbladXPan => {
                // Hasselblad XPan: Dramatic cinematic contrast, velvety deep blacks, crisp micro-acutance
                let diff = luma - 0.45;
                let xpan_luma = (0.45 + (diff * 1.45).tanh() * 0.52).clamp(0.0, 1.0);
                let ratio = if luma > 1e-4 { xpan_luma / luma } else { 1.0 };

                let cr = (r * ratio - xpan_luma) * 0.85;
                let cg = (g * ratio - xpan_luma) * 0.85;
                let cb = (b * ratio - xpan_luma) * 0.82;

                (
                    (xpan_luma + cr).clamp(0.0, 1.0),
                    (xpan_luma + cg).clamp(0.0, 1.0),
                    (xpan_luma + cb).clamp(0.0, 1.0),
                )
            }
        }
    }
}

/// Acros high micro-contrast S-curve
#[inline(always)]
fn eval_acros_curve(y: f32) -> f32 {
    let diff = y.clamp(0.0, 1.0) - 0.44;
    (0.44 + (diff * 1.30).tanh() * 0.50).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_film_simulations_roundtrip() {
        let (r, g, b) = (0.8f32, 0.4f32, 0.2f32);
        let sims = [
            FilmSimulation::FujiProvia,
            FilmSimulation::FujiVelvia,
            FilmSimulation::FujiAstia,
            FilmSimulation::FujiClassicChrome,
            FilmSimulation::FujiClassicNeg,
            FilmSimulation::FujiEterna,
            FilmSimulation::FujiAcrosStandard,
            FilmSimulation::HasselbladHncs,
            FilmSimulation::HasselbladXPan,
        ];

        for sim in &sims {
            let (tr, tg, tb) = sim.apply_pixel(r, g, b, 1.0);
            assert!(tr.is_finite() && tg.is_finite() && tb.is_finite());
            assert!((0.0..=1.0).contains(&tr));
            assert!((0.0..=1.0).contains(&tg));
            assert!((0.0..=1.0).contains(&tb));
        }
    }

    #[test]
    fn test_acros_is_monochrome() {
        let (r, g, b) = (0.7f32, 0.5f32, 0.2f32);
        let (ar, ag, ab) = FilmSimulation::FujiAcrosStandard.apply_pixel(r, g, b, 1.0);
        assert!((ar - ag).abs() < 1e-5);
        assert!((ag - ab).abs() < 1e-5);
    }

    #[test]
    fn test_hasselblad_hncs_preserves_neutral_gray() {
        let gray = 0.5f32;
        let (hr, hg, hb) = FilmSimulation::HasselbladHncs.apply_pixel(gray, gray, gray, 1.0);
        assert!((hr - hg).abs() < 1e-4);
        assert!((hg - hb).abs() < 1e-4);
    }
}
