use std::os::raw::{c_char, c_double, c_float, c_int, c_longlong, c_uchar, c_void};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct COmaRawMetadata {
    pub width: c_int,
    pub height: c_int,
    pub raw_width: c_int,
    pub raw_height: c_int,
    pub make: [c_char; 64],
    pub model: [c_char; 64],
    pub lens: [c_char; 128],
    pub iso: c_float,
    pub shutter: c_float,
    pub aperture: c_float,
    pub focal_len: c_float,
    pub timestamp: c_longlong,
    pub cam_mul: [c_float; 4],
}

#[link(name = "omaraw_shim", kind = "static")]
#[link(name = "raw_r")]
extern "C" {
    pub fn omaraw_open(path: *const c_char, errcode: *mut c_int) -> *mut c_void;
    pub fn omaraw_get_metadata(handle: *mut c_void, meta: *mut COmaRawMetadata) -> c_int;
    pub fn omaraw_extract_thumb_file(handle: *mut c_void, dest_path: *const c_char) -> c_int;
    pub fn omaraw_set_output_space(
        handle: *mut c_void,
        output_color: c_int,
        gamma_power: c_double,
        gamma_slope: c_double,
    ) -> c_int;
    pub fn omaraw_process_image(
        handle: *mut c_void,
        half_size: c_int,
        quality: c_int,
        bps: c_int,
        out_w: *mut c_int,
        out_h: *mut c_int,
        out_colors: *mut c_int,
        out_size: *mut c_int,
    ) -> *mut c_uchar;
    pub fn omaraw_free_image(ptr: *mut c_uchar);
    pub fn omaraw_close(handle: *mut c_void);
}
