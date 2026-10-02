pub mod ffi;

use ffi::*;
use serde::{Deserialize, Serialize};
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
                    if (ptr as usize) % std::mem::align_of::<u16>() == 0 {
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
                if (ptr as usize) % std::mem::align_of::<u16>() == 0 {
                    self.as_slice_u16().to_vec()
                } else {
                    let mut u16_vec = Vec::with_capacity(self.data_size / 2);
                    for chunk in vec[..self.data_size].chunks_exact(2) {
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
        if let BufferStorage::LibRaw(ptr) = self.storage {
            if !ptr.is_null() {
                // SAFETY: ptr was allocated by malloc in C shim and is freed here
                unsafe {
                    omaraw_free_image(ptr);
                }
            }
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
                });
            }
            return Err(format!("LibRaw failed to open {}: code {}", path_ref.display(), errcode));
        }

        Ok(Self {
            backend: RawBackend::LibRaw(handle),
            path: path_ref.to_path_buf(),
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
                });
            }
            return Err(format!("LibRaw failed to open descriptor {}: code {}", fd, errcode));
        }

        Ok(Self {
            backend: RawBackend::LibRaw(handle),
            path: PathBuf::from(proc_path),
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
                thumb
                    .save(dest)
                    .map_err(|e| format!("Failed to save raster thumbnail: {}", e))?;
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
                    let rgb16 = target_img.to_rgb16();
                    let raw_u16 = rgb16.into_raw();
                    let data_size = raw_u16.len() * 2;
                    Ok(ProcessedBuffer {
                        storage: BufferStorage::OwnedU16(raw_u16),
                        width: w,
                        height: h,
                        channels,
                        bits_per_sample: 16,
                        data_size,
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
                    })
                }
            }
            RawBackend::LibRaw(handle) => {
                let mut out_w = 0;
                let mut out_h = 0;
                let mut out_colors = 0;
                let mut out_size = 0;

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

                Ok(ProcessedBuffer {
                    storage: BufferStorage::LibRaw(ptr),
                    width: out_w.max(0) as u32,
                    height: out_h.max(0) as u32,
                    channels: out_colors.max(0) as u32,
                    bits_per_sample: if bps == 16 { 16 } else { 8 },
                    data_size: out_size.max(0) as usize,
                })
            }
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
        if let RawBackend::LibRaw(handle) = self.backend {
            if !handle.is_null() {
                // SAFETY: handle was opened by omaraw_open and is cleanly released here
                unsafe {
                    omaraw_close(handle);
                }
            }
        }
    }
}

impl RawImage {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
