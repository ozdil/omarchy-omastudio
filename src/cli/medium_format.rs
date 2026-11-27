//! Medium-format CLI commands (`medium-format info|lcc-create|multishot|tether`).

use std::path::{Path, PathBuf};

use omastudio_engine::{
    export, icc,
    medium_format::{MediumFormatInfo, hblm, lens, multishot},
    paths, pipeline,
    raw::{self, RawImage},
    security,
};

use super::{ResponseWrapper, print_json};

pub fn run(args: &[String]) {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    // Arguments after the subcommand that are neither flags nor flag values
    let valued = ["--out", "--maker", "--model", "--mount", "--crop"];
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(i, a)| !a.starts_with("--") && !valued.contains(&args[i - 1].as_str()))
        .map(|(_, a)| a)
        .collect();
    let result: Result<serde_json::Value, String> = (|| match args.first().map(String::as_str).unwrap_or("") {
        "info" => {
            let path = Path::new(positional.first().ok_or("Missing RAW file")?);
            let info = MediumFormatInfo::read(path).ok_or("No Hasselblad or Phase One maker data in this file")?;
            let meta = RawImage::open(path).and_then(|r| r.get_metadata()).ok();
            let profile = meta
                .as_ref()
                .and_then(|m| lens::find_profile(lens::profiles(), &m.lens))
                .map(|p| format!("{} {}", p.maker, p.model));
            Ok(serde_json::json!({
                "info": info,
                "lens_profile": profile,
                "flat_field": lens::flat_field_for(path).map(|f| f.source),
            }))
        }
        // A shot of an evenly lit white target (LCC card or diffuser) through the
        // same lens, aperture and shift as the shoot
        "lcc-create" => {
            let path = Path::new(
                positional
                    .first()
                    .ok_or("Usage: medium-format lcc-create <white raw> [--out lcc.json] [--lensfun]")?,
            );
            lens::suspend_corrections();
            let raw = RawImage::open(path)?;
            let buf = raw.process_custom(false, 0, 16)?;
            let (w, h) = (buf.width as usize, buf.height as usize);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            lens::FlatField::check_exposure(buf.as_slice_u16())?;
            let flat = lens::FlatField::from_linear(buf.as_slice_u16(), w, h, raw.developed_flip(), &name)
                .ok_or("Image too small or too dark for a flat field")?;
            let out = flag("--out").map(PathBuf::from).unwrap_or_else(|| path.with_file_name("lcc.json"));
            flat.save(&out)?;
            let k = flat.fit_vignetting();
            let mut lensfun = None;
            if args.iter().any(|a| a == "--lensfun") {
                let meta = raw.get_metadata()?;
                let model = flag("--model").unwrap_or(meta.lens.clone());
                let xml = lens::lensfun_xml(
                    &flag("--maker").unwrap_or(meta.make.clone()),
                    &model,
                    &flag("--mount").unwrap_or_default(),
                    flag("--crop").and_then(|c| c.parse().ok()).unwrap_or(1.0),
                    meta.focal_length,
                    meta.aperture,
                    k,
                );
                let dir = paths::app_data_dir().join("lenses");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let file = dir.join(format!(
                    "{}.xml",
                    model.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect::<String>()
                ));
                security::atomic_write_user(&file, xml.as_bytes())
                    .map_err(|e| format!("Cannot write {}: {}", file.display(), e))?;
                lensfun = Some(file.to_string_lossy().into_owned());
            }
            Ok(serde_json::json!({ "flat_field": out, "vignetting_pa": k, "lensfun_profile": lensfun }))
        }
        "multishot" => {
            if positional.len() != 4 {
                return Err(
                    "Usage: medium-format multishot <frame1> <frame2> <frame3> <frame4> --out merged.tif".into(),
                );
            }
            let out = flag("--out").ok_or("Missing --out")?;
            let raws = positional.iter().map(RawImage::open).collect::<Result<Vec<_>, _>>()?;
            let frames = raws
                .iter()
                .map(|r| r.cfa_frame().ok_or("Frame is not a Bayer raw"))
                .collect::<Result<Vec<_>, _>>()?;
            let (rgb, w, h) =
                multishot::merge_raw_frames(&frames, raw::OutputColorSpace::Srgb).map_err(|e| e.to_string())?;
            let tf = pipeline::transfer::Transfer::srgb();
            let encoded: Vec<u16> = rgb
                .iter()
                .map(|&v| (tf.encode(v as f32 / 65535.0) * 65535.0).round() as u16)
                .collect();
            let icc_bytes = icc::builtin_profile(raw::OutputColorSpace::Srgb).icc().ok();
            let parts = export::tiff_writer::build(
                w as u32,
                h as u32,
                export::tiff_writer::Samples::U16(&encoded),
                export::tiff_writer::Codec::Deflate,
                icc_bytes.as_deref(),
                None,
            )?;
            security::atomic_write_user_parts(Path::new(&out), &parts.slices())
                .map_err(|e| format!("Cannot write {}: {}", out, e))?;
            Ok(serde_json::json!({ "path": out, "width": w, "height": h }))
        }
        "tether" => {
            let sub = positional.first().map(|s| s.as_str()).unwrap_or("list");
            if sub == "list" {
                return serde_json::to_value(hblm::list_devices().map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string());
            }
            if !args.iter().any(|a| a == "--experimental") {
                return Err(
                    "Hasselblad tethering is experimental and untested on cameras; add --experimental to use it".into(),
                );
            }
            let mut cam = hblm::Camera::open().map_err(|e| e.to_string())?;
            let e = |e: hblm::Error| e.to_string();
            match sub {
                "info" => {
                    let (farm, suc) = cam.firmware().map_err(e)?;
                    Ok(serde_json::json!({
                        "firmware": farm,
                        "controller_firmware": suc,
                        "iso": cam.iso().ok(),
                        "exposures_left": cam.exposures_left().ok(),
                    }))
                }
                "iso" => match positional.get(1) {
                    Some(v) => {
                        let iso: u32 = v.parse().map_err(|_| "ISO must be a number")?;
                        cam.set_iso(iso).map_err(e)?;
                        Ok(serde_json::json!({ "iso": iso }))
                    }
                    None => Ok(serde_json::json!({ "iso": cam.iso().map_err(e)? })),
                },
                "capture" => {
                    cam.capture().map_err(e)?;
                    Ok(serde_json::json!({ "captured": true }))
                }
                other => Err(format!("Unknown tether command: {} (list, info, iso, capture)", other)),
            }
        }
        other => Err(format!(
            "Unknown medium-format command: {:?} (info, lcc-create, multishot, tether)",
            other
        )),
    })();
    match result {
        Ok(v) => print_json(&ResponseWrapper::ok(v)),
        Err(e) => print_json::<()>(&ResponseWrapper::err(e)),
    }
}
