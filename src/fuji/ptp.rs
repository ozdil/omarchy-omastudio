//! Fujifilm in-camera RAW conversion and custom settings (C1–C7) over PTP/USB, the
//! protocol X RAW Studio uses. With the camera in "USB RAW CONV./BACKUP RESTORE" mode:
//!
//! 1. Upload the RAF: vendor SendObjectInfo (0x900C, format 0xF802) + SendObject (0x900D).
//! 2. Read the conversion profile the camera derived from it (property 0xD185).
//! 3. Patch the recipe into the profile and write it back (0xD185).
//! 4. Start conversion by writing 0 to 0xD183.
//! 5. Poll GetObjectHandles until the JPEG appears, download it, delete it.
//!
//! The profile is a u16 parameter count followed by fixed-size records; its last
//! `count` × i32 are the conversion parameters, indexed as in [`idx`]. Custom setting
//! slots are selected with 0xD18C and read/written through 0xD18D–0xD1A5.
//! Layouts are documented from interoperability work in FilmKit (MIT) and rawji.

use super::{CameraRecipe, FilmSim, GrainSize, Strength};
use crate::ptp::{self, Error, Result, Session, Transport};
use std::time::{Duration, Instant};

pub const VENDOR_ID: u16 = 0x04CB;

const OP_SEND_OBJECT_INFO: u16 = 0x900C;
const OP_SEND_OBJECT: u16 = 0x900D;
const PROP_START_CONVERSION: u16 = 0xD183;
const PROP_PROFILE: u16 = 0xD185;
const PROP_PRESET_SLOT: u16 = 0xD18C;
const PROP_PRESET_NAME: u16 = 0xD18D;
const FORMAT_RAF: u16 = 0xF802;
/// RAF files up to a 400 MP Pixel Shift composite.
const MAX_RAF: usize = 1024 * 1024 * 1024;

/// Indices of the conversion parameters at the end of the 0xD185 profile.
pub mod idx {
    pub const EXPOSURE_BIAS: usize = 4; // millistops
    pub const DYNAMIC_RANGE: usize = 6; // 100 / 200 / 400
    pub const WIDE_D_RANGE: usize = 7; // D Range Priority
    pub const FILM_SIMULATION: usize = 8;
    pub const GRAIN: usize = 9; // 1 off, 2 weak small, 3 strong small, 4 weak large, 5 strong large
    pub const COLOR_CHROME: usize = 10; // 1 off, 2 weak, 3 strong
    pub const SMOOTH_SKIN: usize = 11;
    pub const WHITE_BALANCE: usize = 12; // 0 = as shot
    pub const WB_SHIFT_R: usize = 13;
    pub const WB_SHIFT_B: usize = 14;
    pub const WB_COLOR_TEMP: usize = 15;
    pub const HIGHLIGHT: usize = 16; // ×10
    pub const SHADOW: usize = 17; // ×10
    pub const COLOR: usize = 18; // ×10
    pub const SHARPNESS: usize = 19; // ×10
    pub const NOISE_REDUCTION: usize = 20; // see nr_code
    pub const COLOR_CHROME_BLUE: usize = 25;
    pub const CLARITY: usize = 27; // ×10
}

/// Film simulation codes used by the profile and preset properties.
pub fn film_sim_code(sim: FilmSim) -> u16 {
    match sim {
        FilmSim::Provia | FilmSim::Other => 0x01,
        FilmSim::Velvia => 0x02,
        FilmSim::Astia => 0x03,
        FilmSim::ProNegHi => 0x04,
        FilmSim::ProNegStd => 0x05,
        FilmSim::Monochrome => 0x06,
        FilmSim::MonochromeYellow => 0x07,
        FilmSim::MonochromeRed => 0x08,
        FilmSim::MonochromeGreen => 0x09,
        FilmSim::Sepia => 0x0A,
        FilmSim::ClassicChrome => 0x0B,
        FilmSim::Acros => 0x0C,
        FilmSim::AcrosYellow => 0x0D,
        FilmSim::AcrosRed => 0x0E,
        FilmSim::AcrosGreen => 0x0F,
        FilmSim::Eterna => 0x10,
        FilmSim::ClassicNeg => 0x11,
        FilmSim::BleachBypass => 0x12,
        FilmSim::NostalgicNeg => 0x13,
        FilmSim::RealaAce => 0x14,
    }
}

pub fn film_sim_from_code(code: u16) -> FilmSim {
    [
        FilmSim::Provia,
        FilmSim::Velvia,
        FilmSim::Astia,
        FilmSim::ProNegHi,
        FilmSim::ProNegStd,
        FilmSim::Monochrome,
        FilmSim::MonochromeYellow,
        FilmSim::MonochromeRed,
        FilmSim::MonochromeGreen,
        FilmSim::Sepia,
        FilmSim::ClassicChrome,
        FilmSim::Acros,
        FilmSim::AcrosYellow,
        FilmSim::AcrosRed,
        FilmSim::AcrosGreen,
        FilmSim::Eterna,
        FilmSim::ClassicNeg,
        FilmSim::BleachBypass,
        FilmSim::NostalgicNeg,
        FilmSim::RealaAce,
    ]
    .into_iter()
    .find(|&s| film_sim_code(s) == code)
    .unwrap_or(FilmSim::Other)
}

/// (white balance name, code)
const WB_CODES: &[(&str, u16)] = &[
    ("As Shot", 0x0000),
    ("Auto", 0x0002),
    ("Daylight", 0x0004),
    ("Incandescent", 0x0006),
    ("Underwater", 0x0008),
    ("Fluorescent", 0x8001),
    ("Shade", 0x8006),
    ("Kelvin", 0x8007),
];

fn wb_code(name: &str) -> u16 {
    WB_CODES.iter().find(|(n, _)| *n == name).map(|&(_, c)| c).unwrap_or(0)
}

fn wb_name(code: u16) -> String {
    match code {
        0x8001..=0x8003 => "Fluorescent".to_string(),
        0x8021 => "Auto".to_string(),
        c => WB_CODES.iter().find(|&&(_, wc)| wc == c).map(|(n, _)| n.to_string()).unwrap_or_else(|| "As Shot".to_string()),
    }
}

/// High ISO NR -4..+4 has its own code table.
const NR_CODES: [(i32, u16); 9] = [
    (-4, 0x8000),
    (-3, 0x7000),
    (-2, 0x4000),
    (-1, 0x3000),
    (0, 0x2000),
    (1, 0x1000),
    (2, 0x0000),
    (3, 0x6000),
    (4, 0x5000),
];

pub fn nr_code(step: i32) -> u16 {
    NR_CODES.iter().find(|(s, _)| *s == step.clamp(-4, 4)).map(|&(_, c)| c).unwrap_or(0x2000)
}

fn nr_step(code: u16) -> i32 {
    NR_CODES.iter().find(|&&(_, c)| c == code).map(|&(s, _)| s).unwrap_or(0)
}

fn effect_code(s: Strength) -> i32 {
    match s {
        Strength::Off => 1,
        Strength::Weak => 2,
        Strength::Strong => 3,
    }
}

fn effect(code: i32) -> Strength {
    match code {
        2 => Strength::Weak,
        3 => Strength::Strong,
        _ => Strength::Off,
    }
}

fn grain_code(s: Strength, size: GrainSize) -> i32 {
    match (s, size) {
        (Strength::Off, _) => 1,
        (Strength::Weak, GrainSize::Small) => 2,
        (Strength::Strong, GrainSize::Small) => 3,
        (Strength::Weak, GrainSize::Large) => 4,
        (Strength::Strong, GrainSize::Large) => 5,
    }
}

fn grain(code: i32) -> (Strength, GrainSize) {
    match code {
        2 => (Strength::Weak, GrainSize::Small),
        3 => (Strength::Strong, GrainSize::Small),
        4 => (Strength::Weak, GrainSize::Large),
        5 => (Strength::Strong, GrainSize::Large),
        _ => (Strength::Off, GrainSize::Small),
    }
}

/// Byte offset of the parameter block of a 0xD185 profile.
fn params_offset(profile: &[u8]) -> Result<usize> {
    let count = u16::from_le_bytes([*profile.first().unwrap_or(&0), *profile.get(1).unwrap_or(&0)]) as usize;
    let size = count * 4;
    if count <= idx::CLARITY || size + 2 > profile.len() {
        return Err(Error::Protocol(format!("unexpected conversion profile ({} bytes, {} parameters)", profile.len(), count)));
    }
    Ok(profile.len() - size)
}

/// Writes a recipe into the camera's conversion profile for the loaded RAF.
/// Parameters the recipe leaves at "as shot" keep the camera's values.
pub fn patch_profile(base: &[u8], r: &CameraRecipe) -> Result<Vec<u8>> {
    let off = params_offset(base)?;
    let mut out = base.to_vec();
    let mut set = |i: usize, v: i32| out[off + i * 4..off + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    set(idx::FILM_SIMULATION, film_sim_code(r.film_simulation) as i32);
    let exp = if r.exposure_bias.is_finite() { r.exposure_bias.clamp(-5.0, 5.0) } else { 0.0 };
    set(idx::EXPOSURE_BIAS, (exp * 1000.0).round() as i32);
    if !r.dynamic_range_auto
        && let Some(dr) = r.dynamic_range
        && [100, 200, 400].contains(&dr)
    {
        set(idx::DYNAMIC_RANGE, dr as i32);
    }
    set(idx::GRAIN, grain_code(r.grain, r.grain_size));
    set(idx::COLOR_CHROME, effect_code(r.color_chrome));
    set(idx::COLOR_CHROME_BLUE, effect_code(r.color_chrome_fx_blue));
    let wb = wb_code(&r.white_balance);
    if wb != 0 {
        set(idx::WHITE_BALANCE, wb as i32);
        if let Some(k) = r.color_temperature.filter(|_| r.white_balance == "Kelvin") {
            set(idx::WB_COLOR_TEMP, k.clamp(2500, 10000) as i32);
        }
    }
    set(idx::WB_SHIFT_R, r.wb_shift.0.clamp(-9, 9));
    set(idx::WB_SHIFT_B, r.wb_shift.1.clamp(-9, 9));
    let hl = if r.highlight.is_finite() { r.highlight.clamp(-2.0, 4.0) } else { 0.0 };
    let sh = if r.shadow.is_finite() { r.shadow.clamp(-2.0, 4.0) } else { 0.0 };
    set(idx::HIGHLIGHT, (hl * 10.0).round() as i32);
    set(idx::SHADOW, (sh * 10.0).round() as i32);
    if !r.film_simulation.is_monochrome() {
        set(idx::COLOR, r.color.clamp(-4, 4) * 10);
    }
    set(idx::SHARPNESS, r.sharpness.clamp(-4, 4) * 10);
    set(idx::NOISE_REDUCTION, nr_code(r.noise_reduction) as i32);
    set(idx::CLARITY, r.clarity.clamp(-5, 5) * 10);
    Ok(out)
}

/// Reads the recipe a conversion profile describes.
pub fn profile_recipe(profile: &[u8]) -> Result<CameraRecipe> {
    let off = params_offset(profile)?;
    let get = |i: usize| i32::from_le_bytes(profile[off + i * 4..off + i * 4 + 4].try_into().unwrap_or_default());
    let sim = film_sim_from_code(get(idx::FILM_SIMULATION) as u16);
    let (g, gs) = grain(get(idx::GRAIN));
    let wb = wb_name(get(idx::WHITE_BALANCE) as u16);
    let dr = get(idx::DYNAMIC_RANGE);
    let wb_temp = get(idx::WB_COLOR_TEMP);
    Ok(CameraRecipe {
        film_simulation: sim,
        film_simulation_name: sim.display_name().to_string(),
        grain: g,
        grain_size: gs,
        color_chrome: effect(get(idx::COLOR_CHROME)),
        color_chrome_fx_blue: effect(get(idx::COLOR_CHROME_BLUE)),
        color_temperature: (wb == "Kelvin" && wb_temp > 0).then_some(wb_temp as u32),
        white_balance: wb,
        wb_shift: (get(idx::WB_SHIFT_R), get(idx::WB_SHIFT_B)),
        dynamic_range: [100, 200, 400].contains(&dr).then_some(dr as u32),
        dynamic_range_auto: dr == 0xFFFF || dr == 0,
        highlight: get(idx::HIGHLIGHT) as f32 / 10.0,
        shadow: get(idx::SHADOW) as f32 / 10.0,
        color: get(idx::COLOR) / 10,
        sharpness: get(idx::SHARPNESS) / 10,
        noise_reduction: nr_step(get(idx::NOISE_REDUCTION) as u16),
        clarity: get(idx::CLARITY) / 10,
        exposure_bias: get(idx::EXPOSURE_BIAS) as f32 / 1000.0,
        ..CameraRecipe::default()
    })
}

/// Custom setting slot properties, in slot order.
mod preset {
    pub const DYNAMIC_RANGE: u16 = 0xD190;
    pub const FILM_SIMULATION: u16 = 0xD192;
    pub const MONO_WC: u16 = 0xD193;
    pub const MONO_MG: u16 = 0xD194;
    pub const GRAIN: u16 = 0xD195;
    pub const COLOR_CHROME: u16 = 0xD196;
    pub const COLOR_CHROME_BLUE: u16 = 0xD197;
    pub const WHITE_BALANCE: u16 = 0xD199;
    pub const WB_SHIFT_R: u16 = 0xD19A;
    pub const WB_SHIFT_B: u16 = 0xD19B;
    pub const COLOR_TEMP: u16 = 0xD19C;
    pub const HIGHLIGHT: u16 = 0xD19D;
    pub const SHADOW: u16 = 0xD19E;
    pub const COLOR: u16 = 0xD19F;
    pub const SHARPNESS: u16 = 0xD1A0;
    pub const NOISE_REDUCTION: u16 = 0xD1A1;
    pub const CLARITY: u16 = 0xD1A2;
}

/// The property writes that store a recipe in a custom setting slot.
pub fn preset_writes(r: &CameraRecipe) -> Vec<(u16, i16)> {
    let hl = if r.highlight.is_finite() { r.highlight.clamp(-2.0, 4.0) } else { 0.0 };
    let sh = if r.shadow.is_finite() { r.shadow.clamp(-2.0, 4.0) } else { 0.0 };
    let mut w = vec![
        (preset::FILM_SIMULATION, film_sim_code(r.film_simulation) as i16),
        (preset::GRAIN, grain_code(r.grain, r.grain_size) as i16),
        (preset::COLOR_CHROME, effect_code(r.color_chrome) as i16),
        (preset::COLOR_CHROME_BLUE, effect_code(r.color_chrome_fx_blue) as i16),
        (preset::WB_SHIFT_R, r.wb_shift.0.clamp(-9, 9) as i16),
        (preset::WB_SHIFT_B, r.wb_shift.1.clamp(-9, 9) as i16),
        (preset::HIGHLIGHT, (hl * 10.0).round() as i16),
        (preset::SHADOW, (sh * 10.0).round() as i16),
        (preset::SHARPNESS, (r.sharpness.clamp(-4, 4) * 10) as i16),
        (preset::NOISE_REDUCTION, nr_code(r.noise_reduction) as i16),
        (preset::CLARITY, (r.clarity.clamp(-5, 5) * 10) as i16),
    ];
    if r.dynamic_range_auto {
        w.push((preset::DYNAMIC_RANGE, 0xFFFFu16 as i16));
    } else {
        let dr = r.dynamic_range.filter(|d| [100, 200, 400].contains(d)).unwrap_or(100);
        w.push((preset::DYNAMIC_RANGE, dr as i16));
    }
    // Preserve "As Shot" (code 0) by leaving the slot's white balance untouched instead of
    // silently overwriting it with Auto.
    let wb = wb_code(&r.white_balance);
    if wb != 0 {
        w.push((preset::WHITE_BALANCE, wb as i16));
    }
    if r.film_simulation.is_monochrome() {
        // The camera refuses 0 for the monochrome toning steps
        let wc = r.mono_warm_cool.clamp(-9, 9);
        if wc != 0 {
            w.push((preset::MONO_WC, (wc * 10) as i16));
        }
        let mg = r.mono_magenta_green.clamp(-9, 9);
        if mg != 0 {
            w.push((preset::MONO_MG, (mg * 10) as i16));
        }
    } else {
        w.push((preset::COLOR, (r.color.clamp(-4, 4) * 10) as i16));
    }
    if let Some(k) = r.color_temperature.filter(|_| r.white_balance == "Kelvin") {
        w.push((preset::COLOR_TEMP, k.clamp(2500, 10000) as i16));
    }
    w
}

/// One custom setting slot read from the camera.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Preset {
    pub slot: u8,
    pub name: String,
    pub recipe: CameraRecipe,
}

/// A connected Fujifilm camera.
pub struct Camera<T: Transport> {
    session: Session<T>,
    pub info: ptp::DeviceInfo,
}

impl Camera<ptp::usb::UsbTransport> {
    /// Opens the first Fujifilm camera on USB.
    pub fn connect() -> Result<Self> {
        Self::open(ptp::usb::UsbTransport::open(Some(VENDOR_ID), None)?)
    }
}

impl<T: Transport> Camera<T> {
    pub fn open(transport: T) -> Result<Self> {
        let mut session = Session::open(transport)?;
        let info = session.device_info()?;
        Ok(Self { session, info })
    }

    pub fn can_convert(&self) -> bool {
        self.info.supports(OP_SEND_OBJECT_INFO) && self.info.has_property(PROP_PROFILE)
    }

    /// Uploads a RAF and returns the conversion profile the camera made for it.
    pub fn load_raf(&mut self, raf: &[u8]) -> Result<Vec<u8>> {
        if !raf.starts_with(b"FUJIFILMCCD-RAW") {
            return Err(Error::Protocol("not a RAF file".into()));
        }
        if raf.len() > MAX_RAF {
            return Err(Error::Protocol("RAF file too large".into()));
        }
        let info = ptp::object_info(FORMAT_RAF, raf.len() as u32, "FUP_FILE.dat");
        self.session.transaction(OP_SEND_OBJECT_INFO, &[0, 0, 0], Some(&info))?;
        self.session.transaction_timeout(OP_SEND_OBJECT, &[], Some(raf), Duration::from_secs(120))?;
        let profile = self.session.prop_value(PROP_PROFILE)?;
        if profile.is_empty() {
            return Err(Error::Protocol("camera returned an empty conversion profile".into()));
        }
        Ok(profile)
    }

    /// Converts the loaded RAF with `profile` and returns the camera's JPEG.
    /// Snapshots existing object handles before triggering conversion so pre-existing
    /// files on the camera's storage card are never downloaded or deleted.
    pub fn convert(&mut self, profile: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let existing: std::collections::HashSet<u32> = self
            .session
            .object_handles(0xFFFF_FFFF, 0, 0)?
            .into_iter()
            .collect();
        self.session.set_prop_value(PROP_PROFILE, profile)?;
        self.session.set_prop_u16(PROP_START_CONVERSION, 0)?;
        let start = Instant::now();
        loop {
            let handles = self.session.object_handles(0xFFFF_FFFF, 0, 0)?;
            if let Some(h) = handles.into_iter().find(|h| !existing.contains(h)) {
                let jpeg = self.session.object(h)?;
                let _ = self.session.delete_object(h);
                return Ok(jpeg);
            }
            if start.elapsed() > timeout {
                return Err(Error::Timeout);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Uploads `raf`, applies `recipe` (or the photo's own settings) and returns the JPEG.
    pub fn convert_raf(&mut self, raf: &[u8], recipe: Option<&CameraRecipe>) -> Result<Vec<u8>> {
        let base = self.load_raf(raf)?;
        let profile = match recipe {
            Some(r) => patch_profile(&base, r)?,
            None => base,
        };
        self.convert(&profile, Duration::from_secs(60))
    }

    fn prop_i16(&mut self, prop: u16) -> Result<i16> {
        Ok(self.session.prop_u16(prop)? as i16)
    }

    /// Reads custom setting slots C1–C7, always restoring the camera's active preset slot.
    pub fn presets(&mut self) -> Result<Vec<Preset>> {
        if !self.info.has_property(PROP_PRESET_SLOT) {
            return Err(Error::Response { op: ptp::op::GET_DEVICE_PROP_VALUE, code: ptp::rc::OPERATION_NOT_SUPPORTED });
        }
        let original = self.session.prop_u16(PROP_PRESET_SLOT).unwrap_or(1);
        let result = (|| {
            let mut out = Vec::new();
            for slot in 1..=7u8 {
                self.session.set_prop_u16(PROP_PRESET_SLOT, slot as u16)?;
                let name = ptp::Reader::new(&self.session.prop_value(PROP_PRESET_NAME)?).string().unwrap_or_default();
                let sim = film_sim_from_code(self.prop_i16(preset::FILM_SIMULATION)? as u16);
                let (g, gs) = grain(self.prop_i16(preset::GRAIN)? as i32);
                let wb = wb_name(self.prop_i16(preset::WHITE_BALANCE)? as u16);
                let dr_raw = self.session.prop_u16(preset::DYNAMIC_RANGE)?;
                let dr = dr_raw as u32;
                let tenth = |v: i16| v as f32 / 10.0;
                let recipe = CameraRecipe {
                    camera: self.info.model.clone(),
                    film_simulation: sim,
                    film_simulation_name: sim.display_name().to_string(),
                    grain: g,
                    grain_size: gs,
                    color_chrome: effect(self.prop_i16(preset::COLOR_CHROME)? as i32),
                    color_chrome_fx_blue: effect(self.prop_i16(preset::COLOR_CHROME_BLUE)? as i32),
                    color_temperature: if wb == "Kelvin" { Some(self.session.prop_u16(preset::COLOR_TEMP)? as u32) } else { None },
                    white_balance: wb,
                    wb_shift: (self.prop_i16(preset::WB_SHIFT_R)? as i32, self.prop_i16(preset::WB_SHIFT_B)? as i32),
                    dynamic_range: [100, 200, 400].contains(&dr).then_some(dr),
                    dynamic_range_auto: dr_raw == 0xFFFF || dr_raw == 0,
                    highlight: tenth(self.prop_i16(preset::HIGHLIGHT)?),
                    shadow: tenth(self.prop_i16(preset::SHADOW)?),
                    color: if sim.is_monochrome() { 0 } else { self.prop_i16(preset::COLOR)? as i32 / 10 },
                    sharpness: self.prop_i16(preset::SHARPNESS)? as i32 / 10,
                    noise_reduction: nr_step(self.session.prop_u16(preset::NOISE_REDUCTION)?),
                    clarity: self.prop_i16(preset::CLARITY)? as i32 / 10,
                    mono_warm_cool: if sim.is_monochrome() { self.prop_i16(preset::MONO_WC)? as i32 / 10 } else { 0 },
                    mono_magenta_green: if sim.is_monochrome() { self.prop_i16(preset::MONO_MG)? as i32 / 10 } else { 0 },
                    ..CameraRecipe::default()
                };
                out.push(Preset { slot, name, recipe });
            }
            Ok(out)
        })();
        let _ = self.session.set_prop_u16(PROP_PRESET_SLOT, original);
        result
    }

    /// Stores a recipe in custom setting slot `slot` (1–7). Always restores the previously
    /// selected slot and returns the properties the camera refused.
    pub fn write_preset(&mut self, slot: u8, name: &str, r: &CameraRecipe) -> Result<Vec<u16>> {
        if !(1..=7).contains(&slot) {
            return Err(Error::Protocol(format!("slot {} is not C1–C7", slot)));
        }
        let original = self.session.prop_u16(PROP_PRESET_SLOT).ok();
        let result = (|| {
            self.session.set_prop_u16(PROP_PRESET_SLOT, slot as u16)?;
            let name: String = name.chars().take(25).collect();
            self.session.set_prop_value(PROP_PRESET_NAME, &ptp::string(&name))?;
            let mut refused = Vec::new();
            for (prop, value) in preset_writes(r) {
                if self.session.set_prop_value(prop, &value.to_le_bytes()).is_err() {
                    refused.push(prop);
                }
            }
            Ok(refused)
        })();
        if let Some(orig) = original {
            let _ = self.session.set_prop_u16(PROP_PRESET_SLOT, orig);
        }
        result
    }

    pub fn close(self) {
        self.session.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 605-byte profile like the camera's: count, header records, 29 parameters.
    fn base_profile() -> Vec<u8> {
        let count = 29u16;
        let mut p = vec![0u8; 605];
        p[..2].copy_from_slice(&count.to_le_bytes());
        p
    }

    #[test]
    fn profile_patch_round_trips_the_recipe() {
        let r = CameraRecipe {
            film_simulation: FilmSim::ClassicNeg,
            grain: Strength::Weak,
            grain_size: GrainSize::Large,
            color_chrome: Strength::Strong,
            color_chrome_fx_blue: Strength::Weak,
            white_balance: "Kelvin".to_string(),
            color_temperature: Some(5600),
            wb_shift: (3, -5),
            dynamic_range: Some(400),
            highlight: -1.5,
            shadow: 2.0,
            color: 3,
            sharpness: -2,
            noise_reduction: -4,
            clarity: -3,
            exposure_bias: 2.0 / 3.0,
            ..CameraRecipe::default()
        };
        let patched = patch_profile(&base_profile(), &r).unwrap();
        assert_eq!(patched.len(), 605);
        let back = profile_recipe(&patched).unwrap();
        assert_eq!(back.film_simulation, FilmSim::ClassicNeg);
        assert_eq!((back.grain, back.grain_size), (Strength::Weak, GrainSize::Large));
        assert_eq!((back.color_chrome, back.color_chrome_fx_blue), (Strength::Strong, Strength::Weak));
        assert_eq!((back.white_balance.as_str(), back.color_temperature, back.wb_shift), ("Kelvin", Some(5600), (3, -5)));
        assert_eq!(back.dynamic_range, Some(400));
        assert_eq!((back.highlight, back.shadow, back.color, back.sharpness, back.noise_reduction, back.clarity), (-1.5, 2.0, 3, -2, -4, -3));
        assert!((back.exposure_bias - 0.667).abs() < 1e-6);
    }

    #[test]
    fn as_shot_white_balance_keeps_the_camera_value() {
        let mut base = base_profile();
        let off = params_offset(&base).unwrap();
        base[off + idx::WHITE_BALANCE * 4..off + idx::WHITE_BALANCE * 4 + 4].copy_from_slice(&4i32.to_le_bytes());
        let r = CameraRecipe { white_balance: "As Shot".into(), ..CameraRecipe::default() };
        assert_eq!(profile_recipe(&patch_profile(&base, &r).unwrap()).unwrap().white_balance, "Daylight");
    }

    #[test]
    fn short_profiles_are_refused() {
        assert!(patch_profile(&[29, 0, 1, 2], &CameraRecipe::default()).is_err());
        assert!(profile_recipe(&[]).is_err());
    }

    #[test]
    fn preset_writes_follow_camera_rules_and_clamp_ranges() {
        let mono = CameraRecipe {
            film_simulation: FilmSim::AcrosRed,
            mono_warm_cool: 2,
            white_balance: "As Shot".into(),
            sharpness: 99,
            clarity: -99,
            wb_shift: (50, -50),
            ..CameraRecipe::default()
        };
        let w = preset_writes(&mono);
        assert!(w.contains(&(preset::MONO_WC, 20)));
        assert!(w.contains(&(preset::SHARPNESS, 40)));
        assert!(w.contains(&(preset::CLARITY, -50)));
        assert!(w.contains(&(preset::WB_SHIFT_R, 9)));
        assert!(w.contains(&(preset::WB_SHIFT_B, -9)));
        assert!(!w.iter().any(|(p, _)| *p == preset::WHITE_BALANCE), "As Shot must not overwrite WB with Auto");
        assert!(!w.iter().any(|(p, _)| *p == preset::MONO_MG || *p == preset::COLOR));
        assert!(preset_writes(&CameraRecipe::default()).contains(&(preset::NOISE_REDUCTION, 0x2000)));
    }

    #[test]
    fn codes_are_consistent() {
        for code in 1..=0x14u16 {
            assert_eq!(film_sim_code(film_sim_from_code(code)), code);
        }
        for step in -4..=4 {
            assert_eq!(nr_step(nr_code(step)), step);
        }
    }

    #[test]
    fn converts_a_raf_over_a_scripted_camera_without_touching_existing_handles() {
        use crate::ptp::{op, tests::Mock};
        let mut m = Mock::default();
        m.reply(op::OPEN_SESSION, 0, None, &[]);
        let mut info = Vec::new();
        info.extend_from_slice(&[100, 0, 6, 0, 0, 0, 100, 0, 0, 0, 0]);
        for arr in [&[OP_SEND_OBJECT_INFO][..], &[], &[PROP_PROFILE], &[], &[]] {
            info.extend_from_slice(&(arr.len() as u32).to_le_bytes());
            for v in arr {
                info.extend_from_slice(&v.to_le_bytes());
            }
        }
        for t in ["FUJIFILM", "X-T30", "1.0", "1"] {
            info.extend_from_slice(&crate::ptp::string(t));
        }
        m.reply(op::GET_DEVICE_INFO, 1, Some(&info), &[]);
        m.reply(OP_SEND_OBJECT_INFO, 2, None, &[]);
        m.reply(OP_SEND_OBJECT, 3, None, &[]);
        m.reply(op::GET_DEVICE_PROP_VALUE, 4, Some(&base_profile()), &[]);
        // Pre-conversion handle snapshot: card already holds handle 5
        m.reply(op::GET_OBJECT_HANDLES, 5, Some(&[1, 0, 0, 0, 5, 0, 0, 0]), &[]);
        m.reply(op::SET_DEVICE_PROP_VALUE, 6, None, &[]);
        m.reply(op::SET_DEVICE_PROP_VALUE, 7, None, &[]);
        // Post-conversion handles: existing 5 plus newly generated 9
        m.reply(op::GET_OBJECT_HANDLES, 8, Some(&[2, 0, 0, 0, 5, 0, 0, 0, 9, 0, 0, 0]), &[]);
        m.reply(op::GET_OBJECT, 9, Some(&[0xFF, 0xD8, 0xFF, 0xD9]), &[]);
        m.reply(op::DELETE_OBJECT, 10, None, &[]);

        let mut cam = Camera::open(m).unwrap();
        assert!(cam.can_convert());
        let recipe = CameraRecipe { film_simulation: FilmSim::Eterna, ..CameraRecipe::default() };
        let jpeg = cam.convert_raf(b"FUJIFILMCCD-RAW 0201", Some(&recipe)).unwrap();
        assert_eq!(jpeg, vec![0xFF, 0xD8, 0xFF, 0xD9]);
    }
}
