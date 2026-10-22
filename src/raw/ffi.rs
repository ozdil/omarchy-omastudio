use std::os::raw::{c_char, c_double, c_float, c_int, c_longlong, c_uchar, c_uint, c_void};

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

#[repr(C)]
#[derive(Debug, Clone)]
pub struct COmaRawCfaInfo {
    pub raw_width: c_int,
    pub raw_height: c_int,
    pub raw_pitch: c_int,
    pub top_margin: c_int,
    pub left_margin: c_int,
    pub width: c_int,
    pub height: c_int,
    pub filters: c_uint,
    pub xtrans: [[c_char; 6]; 6],
    pub colors: c_int,
    pub flip: c_int,
    pub black: [c_float; 4],
    pub black_pattern_h: c_int,
    pub black_pattern_w: c_int,
    pub black_pattern: [c_float; 36],
    pub white: c_float,
    pub cam_mul: [c_float; 4],
    pub pre_mul: [c_float; 4],
    pub rgb_cam: [[c_float; 4]; 3],
}

#[link(name = "omaraw_shim", kind = "static")]
#[link(name = "raw_r")]
unsafe extern "C" {
    pub fn omaraw_open(path: *const c_char, errcode: *mut c_int) -> *mut c_void;
    pub fn omaraw_unpack_cfa(handle: *mut c_void, info: *mut COmaRawCfaInfo) -> *const u16;
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
