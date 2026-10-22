pub mod demosaic;
pub mod develop;
pub mod dng;
pub mod ffi;
pub mod lens;
pub mod pixel_shift;

use crate::pipeline::transfer::Transfer;
use ffi::*;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMetadata {
    pub width: u32,
    pub height: u32,
    pub raw_width: u32,
    pub raw_height: u32,
    pub make: String,
    pub model: String,
    pub lens: String,
    pub iso: f32,
    pub shutter: f32,
    pub aperture: f32,
    pub focal_length: f32,
    pub timestamp: i64,
    pub cam_mul: [f32; 4],
}

pub enum BufferStorage {
    LibRaw(*mut u8),
    OwnedU8(Vec<u8>),
    OwnedU16(Vec<u16>),
}

pub struct ProcessedBuffer {
    storage: BufferStorage,
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub bits_per_sample: u32,
    pub data_size: usize,
    /// Display curve of this buffer's colour space. 16-bit buffers hold linear light
    /// and the pipeline encodes them with this curve; 8-bit buffers are already encoded.
    pub transfer: Transfer,
}

// SAFETY: The buffer is exclusively owned and safe to pass across threads
unsafe impl Send for ProcessedBuffer {}
unsafe impl Sync for ProcessedBuffer {}

impl ProcessedBuffer {
    pub fn as_slice(&self) -> &[u8] {
        if self.data_size == 0 {
            &[]
        } else {
            match &self.storage {
                BufferStorage::LibRaw(ptr) => {
                    if ptr.is_null() {
                        &[]
                    } else {
                        // SAFETY: ptr points to valid heap memory of data_size allocated by C shim
                        unsafe { std::slice::from_raw_parts(*ptr, self.data_size) }
                    }
                }
                BufferStorage::OwnedU8(vec) => &vec[..self.data_size.min(vec.len())],
                BufferStorage::OwnedU16(vec) => {
                    // SAFETY: Vec<u16> cast to &[u8] slice of byte length data_size
                    unsafe {
                        std::slice::from_raw_parts(vec.as_ptr() as *const u8, self.data_size.min(vec.len() * 2))
                    }
                }
            }
        }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }

    /// Converts a 1-channel buffer into an owned interleaved 3-channel buffer.
    /// Applies radial lens corrections. 16-bit buffers are linear light; 8-bit ones are
    /// encoded with the buffer's curve.
    fn with_lens_correction(self, corr: &lens::RadialCorrection) -> ProcessedBuffer {
        let (w, h) = (self.width as usize, self.height as usize);
        let (storage, data_size) = if self.bits_per_sample == 16 {
            let out = corr.apply_linear_u16(&self.as_slice_u16()[..w * h * 3], w, h);
            let len = out.len() * 2;
            (BufferStorage::OwnedU16(out), len)
        } else {
            let out = corr.apply_encoded_u8(&self.as_slice()[..w * h * 3], w, h, self.transfer);
            let len = out.len();
            (BufferStorage::OwnedU8(out), len)
        };
        ProcessedBuffer { storage, data_size, ..self }
    }

    fn expand_gray_to_rgb(self) -> ProcessedBuffer {
        let pixels = (self.width as usize) * (self.height as usize);
        let (storage, data_size) = if self.bits_per_sample == 16 {
            let src = self.as_slice_u16();
            let out: Vec<u16> = src.iter().take(pixels).flat_map(|&v| [v, v, v]).collect();
            let len = out.len() * 2;
            (BufferStorage::OwnedU16(out), len)
        } else {
            let src = self.as_slice();
            let out: Vec<u8> = src.iter().take(pixels).flat_map(|&v| [v, v, v]).collect();
            let len = out.len();
            (BufferStorage::OwnedU8(out), len)
        };
        ProcessedBuffer {
            storage,
            width: self.width,
            height: self.height,
            channels: 3,
            bits_per_sample: self.bits_per_sample,
            data_size,
            transfer: self.transfer,
        }
    }

    pub fn as_slice_u16(&self) -> &[u16] {
        if self.data_size < 2 {
            &[]
        } else {
            match &self.storage {
                BufferStorage::LibRaw(ptr) => {
                    if ptr.is_null() {
                        &[]
                    } else {
                        // SAFETY: ptr was allocated by malloc in C shim, aligned to at least 8/16 bytes
                        unsafe {
                            std::slice::from_raw_parts(*ptr as *const u16, self.data_size / 2)
                        }
                    }
                }
                BufferStorage::OwnedU16(vec) => &vec[..(self.data_size / 2).min(vec.len())],
                BufferStorage::OwnedU8(vec) => {
                    let ptr = vec.as_ptr();
                    if (ptr as usize).is_multiple_of(std::mem::align_of::<u16>()) {
                        // SAFETY: vec pointer is 2-byte aligned and contains data_size bytes
                        unsafe {
                            std::slice::from_raw_parts(ptr as *const u16, self.data_size / 2)
                        }
                    } else {
                        &[]
                    }
                }
            }
        }
    }

    pub fn to_vec_u16(&self) -> Vec<u16> {
        match &self.storage {
            BufferStorage::OwnedU16(vec) => vec[..(self.data_size / 2).min(vec.len())].to_vec(),
            BufferStorage::OwnedU8(vec) => {
                let ptr = vec.as_ptr();
                if (ptr as usize).is_multiple_of(std::mem::align_of::<u16>()) {
                    self.as_slice_u16().to_vec()
                } else {
                    let mut u16_vec = Vec::with_capacity(self.data_size / 2);
                    for chunk in vec[..self.data_size].as_chunks::<2>().0 {
                        u16_vec.push(u16::from_ne_bytes([chunk[0], chunk[1]]));
                    }
                    u16_vec
                }
            }
            BufferStorage::LibRaw(_) => self.as_slice_u16().to_vec(),
        }
    }
}

impl Drop for ProcessedBuffer {
    fn drop(&mut self) {
        if let BufferStorage::LibRaw(ptr) = self.storage
            && !ptr.is_null()
        {
            // SAFETY: ptr was allocated by malloc in C shim and is freed here
            unsafe {
                omaraw_free_image(ptr);
            }
        }
    }
}

/// RGB spaces LibRaw can render directly, with the transfer curve each ICC expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputColorSpace {
    Srgb,
    AdobeRgb,
    ProPhotoRgb,
    DisplayP3,
    Rec2020,
}

impl OutputColorSpace {
    /// Maps an export ICC profile id ("AdobeRGB1998", "DisplayP3", ...) to a space.
    pub fn from_profile_name(name: &str) -> Option<Self> {
        let n = name.to_lowercase().replace([' ', '-', '_', '.'], "");
        if n.contains("adobe") || n.contains("a98") {
            Some(Self::AdobeRgb)
        } else if n.contains("prophoto") || n.contains("romm") {
            Some(Self::ProPhotoRgb)
        } else if n.contains("p3") {
            Some(Self::DisplayP3)
        } else if n.contains("2020") {
            Some(Self::Rec2020)
        } else if n.contains("srgb") {
            Some(Self::Srgb)
        } else {
            None
        }
    }

    /// Display curve of this space (the one its ICC profile expects).
    pub fn transfer(self) -> Transfer {
        let (_, power, slope) = self.libraw_params();
        Transfer::from_libraw(power, slope)
    }

    /// (LibRaw output_color, gamm[0], gamm[1])
    fn libraw_params(self) -> (i32, f64, f64) {
        match self {
            // Keep LibRaw's default BT.709 curve so exports match the viewport preview
            Self::Srgb => (1, 0.45, 4.5),
            Self::AdobeRgb => (2, 1.0 / 2.199_218_75, 0.0),
            Self::ProPhotoRgb => (4, 1.0 / 1.8, 0.0),
            Self::DisplayP3 => (7, 1.0 / 2.4, 12.92),
            Self::Rec2020 => (8, 0.45, 4.5),
        }
    }
}

/// Who turns sensor data into RGB for 16-bit (linear) decodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemosaicEngine {
    /// OmaStudio's own development ([`develop`]); falls back to LibRaw for sensors it
    /// does not handle (Foveon, linear DNG, Fuji SuperCCD, 4-colour CFAs).
    Own,
    /// LibRaw's `dcraw_process`.
    LibRaw,
}

impl DemosaicEngine {
    /// `OMASTUDIO_DEMOSAIC=libraw` switches back to LibRaw's processing.
    pub fn from_env() -> Self {
        match std::env::var("OMASTUDIO_DEMOSAIC") {
            Ok(v) if v.eq_ignore_ascii_case("libraw") => Self::LibRaw,
            _ => Self::Own,
        }
    }
}

pub enum RawBackend {
    LibRaw(*mut std::os::raw::c_void),
    Raster {
        img: image::DynamicImage,
    },
}

pub struct RawImage {
    backend: RawBackend,
    path: PathBuf,
    /// Colour space the next LibRaw decode renders into (sRGB unless an export picks another).
    output_space: Cell<OutputColorSpace>,
    /// Lens corrections recorded by the camera, read on first decode.
    lens: std::cell::OnceCell<Option<lens::RadialCorrection>>,
}

// SAFETY: RawImage handle/backend is managed exclusively by Rust wrapper
unsafe impl Send for RawImage {}

fn is_raster_extension(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(
            ext.to_lowercase().as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "tif" | "tiff"
        )
    } else {
        false
    }
}

impl RawImage {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path_ref = path.as_ref();
        crate::security::verify_safe_file(path_ref)
            .map_err(|e| format!("Security check failed for {}: {}", path_ref.display(), e))?;

        if is_raster_extension(path_ref) {
            let img = image::open(path_ref)
                .map_err(|e| format!("Failed to open raster image {}: {}", path_ref.display(), e))?;
            return Ok(Self {
                backend: RawBackend::Raster { img },
                path: path_ref.to_path_buf(),
                output_space: Cell::new(OutputColorSpace::Srgb),
                lens: std::cell::OnceCell::new(),
            });
        }

        let c_path = CString::new(path_ref.to_string_lossy().as_bytes())
            .map_err(|e| format!("Invalid path string: {}", e))?;

        let mut errcode: std::os::raw::c_int = 0;
        // SAFETY: c_path is a null-terminated C string, errcode is a valid pointer
        let handle = unsafe { omaraw_open(c_path.as_ptr(), &mut errcode) };

        if handle.is_null() {
            // Fallback: try loading with standard image reader if LibRaw fails
            if let Ok(img) = image::open(path_ref) {
                return Ok(Self {
                    backend: RawBackend::Raster { img },
                    path: path_ref.to_path_buf(),
                    output_space: Cell::new(OutputColorSpace::Srgb),
                    lens: std::cell::OnceCell::new(),
                });
            }
            return Err(format!("LibRaw failed to open {}: code {}", path_ref.display(), errcode));
        }

        Ok(Self {
            backend: RawBackend::LibRaw(handle),
            path: path_ref.to_path_buf(),
            output_space: Cell::new(OutputColorSpace::Srgb),
            lens: std::cell::OnceCell::new(),
        })
    }

    /// Opens an image directly from a verified open file descriptor via /proc/self/fd/{fd}
    /// without releasing the descriptor or following mutable disk paths.
    pub fn open_from_file(file: &std::fs::File) -> Result<Self, String> {
        use std::os::unix::io::AsRawFd;

        let fd = file.as_raw_fd();
        let proc_path = format!("/proc/self/fd/{}", fd);
        let c_path = CString::new(proc_path.as_bytes())
            .map_err(|e| format!("Invalid proc path string: {}", e))?;

        let mut errcode: std::os::raw::c_int = 0;
        // SAFETY: c_path is a valid null-terminated C string, errcode is a valid pointer
        let handle = unsafe { omaraw_open(c_path.as_ptr(), &mut errcode) };

        if handle.is_null() {
            // Fallback: try reading file descriptor via image crate
            if let Ok(img) = image::open(&proc_path) {
                return Ok(Self {
                    backend: RawBackend::Raster { img },
                    path: PathBuf::from(proc_path),
                    output_space: Cell::new(OutputColorSpace::Srgb),
                    lens: std::cell::OnceCell::new(),
                });
            }
            return Err(format!("LibRaw failed to open descriptor {}: code {}", fd, errcode));
        }

        Ok(Self {
            backend: RawBackend::LibRaw(handle),
            path: PathBuf::from(proc_path),
            output_space: Cell::new(OutputColorSpace::Srgb),
            lens: std::cell::OnceCell::new(),
        })
    }

    pub fn get_metadata(&self) -> Result<RawMetadata, String> {
        match &self.backend {
            RawBackend::Raster { img } => {
                let (w, h) = (img.width(), img.height());
                let ext = self
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("IMG")
                    .to_uppercase();

                Ok(RawMetadata {
                    width: w,
                    height: h,
                    raw_width: w,
                    raw_height: h,
                    make: "Standard Image".to_string(),
                    model: ext,
                    lens: "Standard Lens".to_string(),
                    iso: 100.0,
                    shutter: 0.01,
                    aperture: 2.8,
                    focal_length: 50.0,
                    timestamp: 0,
                    cam_mul: [1.0, 1.0, 1.0, 1.0],
                })
            }
            RawBackend::LibRaw(handle) => {
                let mut cmeta = COmaRawMetadata {
                    width: 0,
                    height: 0,
                    raw_width: 0,
                    raw_height: 0,
                    make: [0; 64],
                    model: [0; 64],
                    lens: [0; 128],
                    iso: 0.0,
                    shutter: 0.0,
                    aperture: 0.0,
                    focal_len: 0.0,
                    timestamp: 0,
                    cam_mul: [1.0, 1.0, 1.0, 1.0],
                };

                // SAFETY: handle is valid and cmeta is allocated on stack
                let ret = unsafe { omaraw_get_metadata(*handle, &mut cmeta) };
                if ret != 0 {
                    return Err("Failed to extract metadata from RAW file".to_string());
                }

                let make = unsafe { CStr::from_ptr(cmeta.make.as_ptr()) }
                    .to_string_lossy()
                    .trim()
                    .to_string();
                let model = unsafe { CStr::from_ptr(cmeta.model.as_ptr()) }
                    .to_string_lossy()
                    .trim()
                    .to_string();
                let lens = unsafe { CStr::from_ptr(cmeta.lens.as_ptr()) }
                    .to_string_lossy()
                    .trim()
                    .to_string();

                Ok(RawMetadata {
                    width: cmeta.width.max(0) as u32,
                    height: cmeta.height.max(0) as u32,
                    raw_width: cmeta.raw_width.max(0) as u32,
                    raw_height: cmeta.raw_height.max(0) as u32,
                    make,
                    model,
                    lens,
                    iso: cmeta.iso,
                    shutter: cmeta.shutter,
                    aperture: cmeta.aperture,
                    focal_length: cmeta.focal_len,
                    timestamp: cmeta.timestamp,
                    cam_mul: cmeta.cam_mul,
                })
            }
        }
    }

    pub fn extract_thumbnail<P: AsRef<Path>>(&self, dest_path: P) -> Result<(), String> {
        let dest = dest_path.as_ref();
        if let Some(parent) = dest.parent() {
            crate::security::ensure_secure_dir(parent)
                .map_err(|e| format!("Failed to ensure thumbnail directory: {}", e))?;
        }

        match &self.backend {
            RawBackend::Raster { img } => {
                let thumb = img.thumbnail(400, 400);
                let mut buf = std::io::Cursor::new(Vec::new());
                let ext = dest
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("jpg")
                    .to_lowercase();
                let fmt = match ext.as_str() {
                    "png" => image::ImageFormat::Png,
                    "webp" => image::ImageFormat::WebP,
                    _ => image::ImageFormat::Jpeg,
                };
                thumb
                    .write_to(&mut buf, fmt)
                    .map_err(|e| format!("Failed to encode raster thumbnail: {}", e))?;
                crate::security::atomic_write_secure(dest, &buf.into_inner())
                    .map_err(|e| format!("Failed to securely save raster thumbnail: {}", e))?;
                Ok(())
            }
            RawBackend::LibRaw(handle) => {
                let c_dest = CString::new(dest.to_string_lossy().as_bytes())
                    .map_err(|e| format!("Invalid destination path: {}", e))?;

                // SAFETY: handle is valid, c_dest is valid C string
                let ret = unsafe { omaraw_extract_thumb_file(*handle, c_dest.as_ptr()) };
                if ret != 0 {
                    return Err(format!("Failed to extract embedded thumbnail: code {}", ret));
                }

                Ok(())
            }
        }
    }

    pub fn process_custom(&self, half_size: bool, quality: i32, bps: i32) -> Result<ProcessedBuffer, String> {
        self.process_with(DemosaicEngine::from_env(), half_size, quality, bps)
    }

    /// Like [`RawImage::process_custom`] with an explicit demosaic engine.
    ///
    /// Lens corrections recorded in the file (Fujifilm RAF data, DNG OpcodeList3) are
    /// applied to every decode, as the camera does for its own JPEGs. `OMASTUDIO_LENS_CORRECTION=off`
    /// turns them off.
    pub fn process_with(
        &self,
        engine: DemosaicEngine,
        half_size: bool,
        quality: i32,
        bps: i32,
    ) -> Result<ProcessedBuffer, String> {
        let buf = self.process_uncorrected(engine, half_size, quality, bps)?;
        match self.lens_correction() {
            Some(corr) if buf.channels == 3 => Ok(buf.with_lens_correction(corr)),
            _ => Ok(buf),
        }
    }

    /// Exposure the camera adds for its own rendering of this photo, in EV (0 when the
    /// file records none). `OMASTUDIO_BASELINE_EXPOSURE=off` ignores it.
    pub fn baseline_exposure(&self) -> f32 {
        if std::env::var("OMASTUDIO_BASELINE_EXPOSURE").is_ok_and(|v| v == "off") {
            return 0.0;
        }
        match &self.backend {
            // SAFETY: handle is a valid open LibRaw handle owned by self
            RawBackend::LibRaw(handle) => unsafe { omaraw_baseline_ev(*handle) },
            RawBackend::Raster { .. } => 0.0,
        }
    }

    /// Lens corrections recorded in the file, if any and not turned off.
    pub fn lens_correction(&self) -> Option<&lens::RadialCorrection> {
        if !matches!(self.backend, RawBackend::LibRaw(_))
            || std::env::var("OMASTUDIO_LENS_CORRECTION").is_ok_and(|v| v == "off" || v == "0")
        {
            return None;
        }
        self.lens.get_or_init(|| crate::fuji::lens::read(&self.path).or_else(|| dng::read_corrections(&self.path))).as_ref()
    }

    fn process_uncorrected(
        &self,
        engine: DemosaicEngine,
        half_size: bool,
        quality: i32,
        bps: i32,
    ) -> Result<ProcessedBuffer, String> {
        if engine == DemosaicEngine::Own
            && bps == 16
            && let Some(buf) = self.develop_own(half_size)
        {
            return Ok(buf);
        }
        match &self.backend {
            RawBackend::Raster { img } => {
                let target_img = if half_size {
                    let w = (img.width() / 2).max(1);
                    let h = (img.height() / 2).max(1);
                    img.resize_exact(w, h, image::imageops::FilterType::Triangle)
                } else {
                    img.clone()
                };

                let w = target_img.width();
                let h = target_img.height();
                let channels = 3u32;

                if bps == 16 {
                    // 16-bit buffers are linear light: undo the file's sRGB curve.
                    let srgb = Transfer::srgb();
                    let lut: Vec<u16> = (0..=u16::MAX)
                        .map(|v| (srgb.decode(v as f32 / 65535.0) * 65535.0).round().clamp(0.0, 65535.0) as u16)
                        .collect();
                    let mut raw_u16 = target_img.to_rgb16().into_raw();
                    for v in raw_u16.iter_mut() {
                        *v = lut[*v as usize];
                    }
                    let data_size = raw_u16.len() * 2;
                    Ok(ProcessedBuffer {
                        storage: BufferStorage::OwnedU16(raw_u16),
                        width: w,
                        height: h,
                        channels,
                        bits_per_sample: 16,
                        data_size,
                        transfer: Transfer::srgb(),
                    })
                } else {
                    let rgb8 = target_img.to_rgb8();
                    let raw_vec = rgb8.into_raw();
                    let data_size = raw_vec.len();
                    Ok(ProcessedBuffer {
                        storage: BufferStorage::OwnedU8(raw_vec),
                        width: w,
                        height: h,
                        channels,
                        bits_per_sample: 8,
                        data_size,
                        transfer: Transfer::srgb(),
                    })
                }
            }
            RawBackend::LibRaw(handle) => {
                let mut out_w = 0;
                let mut out_h = 0;
                let mut out_colors = 0;
                let mut out_size = 0;

                // 16-bit decodes are linear light; the pipeline applies the space's curve after
                // white balance and exposure. 8-bit decodes keep LibRaw's encoded output.
                let space = self.output_space.get();
                let (code, power, slope) = space.libraw_params();
                let (power, slope) = if bps == 16 { (1.0, 1.0) } else { (power, slope) };
                // SAFETY: handle is a valid open LibRaw handle owned by self
                if unsafe { omaraw_set_output_space(*handle, code, power, slope) } != 0 {
                    return Err("Failed to configure RAW output space".to_string());
                }

                let hs = if half_size { 1 } else { 0 };
                // SAFETY: handle is valid, all output pointers point to stack variables
                let ptr = unsafe {
                    omaraw_process_image(
                        *handle,
                        hs,
                        quality,
                        bps,
                        &mut out_w,
                        &mut out_h,
                        &mut out_colors,
                        &mut out_size,
                    )
                };

                if ptr.is_null() || out_size <= 0 {
                    return Err("Failed to process RAW image".to_string());
                }

                let buf = ProcessedBuffer {
                    storage: BufferStorage::LibRaw(ptr),
                    width: out_w.max(0) as u32,
                    height: out_h.max(0) as u32,
                    channels: out_colors.max(0) as u32,
                    bits_per_sample: if bps == 16 { 16 } else { 8 },
                    data_size: out_size.max(0) as usize,
                    transfer: space.transfer(),
                };

                match buf.channels {
                    3 => Ok(buf),
                    // Monochrome sensors (Leica Monochrom, some DNGs) decode to 1 channel; the
                    // whole pipeline assumes interleaved RGB, so replicate the sample.
                    1 => Ok(buf.expand_gray_to_rgb()),
                    n => Err(format!("Unsupported channel count from decoder: {}", n)),
                }
            }
        }
    }

    /// Develops a CFA RAW with OmaStudio's own pipeline into linear 16-bit RGB, or
    /// returns None when the source is not a plain 3-colour CFA.
    /// Black-subtracted, white-balanced sensor data of a plain 3-colour CFA (Bayer or
    /// X-Trans), or None for other sensors and raster files.
    pub fn cfa_frame(&self) -> Option<develop::CfaFrame> {
        let RawBackend::LibRaw(handle) = &self.backend else {
            return None;
        };
        // SAFETY: COmaRawCfaInfo is plain old data; all-zero is a valid value
        let mut info: COmaRawCfaInfo = unsafe { std::mem::zeroed() };
        // SAFETY: handle is a valid open LibRaw handle owned by self; info is a valid pointer
        let raw = unsafe { omaraw_unpack_cfa(*handle, &mut info) };
        // SAFETY: the shim returns the handle's raw plane (raw_pitch * raw_height values),
        // which stays valid while self (and so the handle) is alive
        unsafe { develop::CfaFrame::from_libraw(raw, &info) }
    }

    fn develop_own(&self, half_size: bool) -> Option<ProcessedBuffer> {
        let frame = self.cfa_frame()?;
        // Binning an X-Trans sensor gives a 1/3-size preview; LibRaw's half-size decode
        // is larger and renders the same colours, so previews keep using it
        if half_size && matches!(frame.pattern, demosaic::CfaPattern::XTrans(_)) {
            return None;
        }
        let space = self.output_space.get();
        let (data, width, height) = frame.develop(space, half_size);
        let data_size = data.len() * 2;
        Some(ProcessedBuffer {
            storage: BufferStorage::OwnedU16(data),
            width,
            height,
            channels: 3,
            bits_per_sample: 16,
            data_size,
            // Linear data; the pipeline encodes it with the output space's curve
            transfer: space.transfer(),
        })
    }

    /// Selects the RGB output space for the next decode. Only LibRaw sources can be
    /// converted; raster sources stay sRGB and return false.
    pub fn set_output_space(&self, space: OutputColorSpace) -> bool {
        match &self.backend {
            RawBackend::LibRaw(_) => {
                self.output_space.set(space);
                true
            }
            RawBackend::Raster { .. } => space == OutputColorSpace::Srgb,
        }
    }

    pub fn process_preview(&self, half_size: bool) -> Result<ProcessedBuffer, String> {
        self.process_custom(half_size, 0, 8)
    }

    pub fn process_preview_16(&self, half_size: bool) -> Result<ProcessedBuffer, String> {
        self.process_custom(half_size, 0, 16)
    }

    pub fn process_full(&self, quality: i32) -> Result<ProcessedBuffer, String> {
        self.process_custom(false, quality, 8)
    }

    pub fn process_full_16(&self, quality: i32) -> Result<ProcessedBuffer, String> {
        self.process_custom(false, quality, 16)
    }
}

impl Drop for RawImage {
    fn drop(&mut self) {
        if let RawBackend::LibRaw(handle) = self.backend
            && !handle.is_null()
        {
            // SAFETY: handle was opened by omaraw_open and is cleanly released here
            unsafe {
                omaraw_close(handle);
            }
        }
    }
}

impl RawImage {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
