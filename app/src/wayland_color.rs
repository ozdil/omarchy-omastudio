//! Zero-dependency Wayland `wp-color-management-v1` (version 1) display colour-space
//! and HDR capability probe (`WP-16`, `BUG-FE-15`).
//!
//! Runs once before `QGuiApplication::new()` so Qt Quick's default `QSurfaceFormat` and
//! `QColorSpace` can request a 10-bit wide-gamut or FP16 HDR swapchain on supporting
//! Wayland compositors (Hyprland, KWin/Plasma 6.3+, Mutter/GNOME 48+) while staying
//! 8-bit sRGB on standard SDR/X11/headless sessions.

use std::collections::VecDeque;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const PROBE_TIMEOUT: Duration = Duration::from_millis(150);
const MAX_OUTPUTS: usize = 8;

// Wayland core object IDs & opcodes.
const WL_DISPLAY_ID: u32 = 1;
const WL_DISPLAY_SYNC_OP: u16 = 0;
const WL_DISPLAY_GET_REGISTRY_OP: u16 = 1;
const WL_DISPLAY_ERROR_EVT: u16 = 0;
const WL_REGISTRY_BIND_OP: u16 = 0;
const WL_REGISTRY_GLOBAL_EVT: u16 = 0;
const WL_CALLBACK_DONE_EVT: u16 = 0;

// `wp-color-management-v1` (version 1) opcodes & enum values.
const WP_COLOR_MANAGER_IFACE: &str = "wp_color_manager_v1";
const WL_OUTPUT_IFACE: &str = "wl_output";
const WP_COLOR_MANAGER_DESTROY_OP: u16 = 0;
const WP_COLOR_MANAGER_GET_OUTPUT_OP: u16 = 1;
const WP_CM_OUTPUT_DESTROY_OP: u16 = 0;
const WP_CM_OUTPUT_GET_IMAGE_DESC_OP: u16 = 1;
const WP_IMG_DESC_DESTROY_OP: u16 = 0;
const WP_IMG_DESC_GET_INFO_OP: u16 = 1;
const WP_IMG_DESC_FAILED_EVT: u16 = 0;
const WP_IMG_DESC_READY_EVT: u16 = 1;

const WP_IMG_INFO_DONE_EVT: u16 = 0;
const WP_IMG_INFO_ICC_FILE_EVT: u16 = 1;
const WP_IMG_INFO_PRIMARIES_EVT: u16 = 2;
const WP_IMG_INFO_PRIMARIES_NAMED_EVT: u16 = 3;
const WP_IMG_INFO_TF_NAMED_EVT: u16 = 5;
const WP_IMG_INFO_LUMINANCES_EVT: u16 = 6;
const WP_IMG_INFO_TARGET_LUMINANCE_EVT: u16 = 8;
const WP_IMG_INFO_TARGET_MAX_CLL_EVT: u16 = 9;

// `wp_color_manager_v1::primaries` enum.
const PRIMARIES_SRGB: u32 = 1;
const PRIMARIES_BT2020: u32 = 6;
const PRIMARIES_DCI_P3: u32 = 8;
const PRIMARIES_DISPLAY_P3: u32 = 9;

// `wp_color_manager_v1::transfer_function` enum.
const TF_EXT_LINEAR: u32 = 5;
const TF_EXT_SRGB: u32 = 10;
const TF_ST2084_PQ: u32 = 11;
const TF_HLG: u32 = 13;

/// Active display colour capability negotiated for the application window and engine previews.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayColorInfo {
    /// True when the Wayland compositor advertised `wp_color_manager_v1`.
    pub protocol_supported: bool,
    /// Colour space tag shared with `omastudio-engine` (`"srgb"`, `"display_p3"`, `"bt2020"`,
    /// `"bt2100_pq"`, or `"bt2100_hlg"`).
    pub color_space: String,
    /// Numeric `ColorSpaceTag` / Qt `QColorSpace` selector (`0..=4`).
    pub color_space_code: i32,
    /// Request a 10-bit (`10/10/10/2`) swapchain surface format.
    pub prefer_10bit: bool,
    /// Request an FP16 (`16/16/16/16` + `FloatColorBuffer`) HDR swapchain surface format.
    pub prefer_hdr_float: bool,
    /// True when an HDR output or explicit `QSG_RHI_HDR` mode is active.
    pub hdr_active: bool,
    /// Peak display luminance in nits (cd/m²), or `0` when unreported.
    pub max_luminance_nits: u32,
    /// Reference SDR white luminance in nits (cd/m²).
    pub sdr_white_nits: u32,
}

impl Default for DisplayColorInfo {
    fn default() -> Self {
        Self {
            protocol_supported: false,
            color_space: "srgb".to_string(),
            color_space_code: 0,
            prefer_10bit: false,
            prefer_hdr_float: false,
            hdr_active: false,
            max_luminance_nits: 0,
            sdr_white_nits: 80,
        }
    }
}

/// Returns the process-wide detected display colour capabilities (probed once at startup).
pub fn detected() -> &'static DisplayColorInfo {
    static INFO: OnceLock<DisplayColorInfo> = OnceLock::new();
    INFO.get_or_init(detect_now)
}

fn detect_now() -> DisplayColorInfo {
    if let Ok(val) = std::env::var("QSG_RHI_HDR")
        && let Some(overridden) = parse_hdr_env_override(&val)
    {
        return overridden;
    }
    if std::env::var("QT_QUICK_BACKEND")
        .ok()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("software"))
    {
        return DisplayColorInfo::default();
    }
    let Some(sock_path) = wayland_socket_path() else {
        return DisplayColorInfo::default();
    };
    let Ok(mut stream) = UnixStream::connect(&sock_path) else {
        return DisplayColorInfo::default();
    };
    probe_stream(&mut stream).unwrap_or_default()
}

fn parse_hdr_env_override(raw: &str) -> Option<DisplayColorInfo> {
    let v = raw.trim().to_ascii_lowercase();
    if v.is_empty() {
        return None;
    }
    match v.as_str() {
        "off" | "0" | "false" | "none" | "sdr" | "srgb" => Some(DisplayColorInfo::default()),
        "scrgb" | "fp16" | "float" => Some(DisplayColorInfo {
            protocol_supported: false,
            color_space: "display_p3".to_string(),
            color_space_code: 1,
            prefer_10bit: true,
            prefer_hdr_float: true,
            hdr_active: true,
            max_luminance_nits: 1000,
            sdr_white_nits: 203,
        }),
        "hdr10" | "pq" | "bt2100_pq" => Some(DisplayColorInfo {
            protocol_supported: false,
            color_space: "bt2100_pq".to_string(),
            color_space_code: 3,
            prefer_10bit: true,
            prefer_hdr_float: false,
            hdr_active: true,
            max_luminance_nits: 1000,
            sdr_white_nits: 203,
        }),
        "hlg" | "bt2100_hlg" => Some(DisplayColorInfo {
            protocol_supported: false,
            color_space: "bt2100_hlg".to_string(),
            color_space_code: 4,
            prefer_10bit: true,
            prefer_hdr_float: false,
            hdr_active: true,
            max_luminance_nits: 1000,
            sdr_white_nits: 203,
        }),
        "p3" | "display_p3" => Some(DisplayColorInfo {
            protocol_supported: false,
            color_space: "display_p3".to_string(),
            color_space_code: 1,
            prefer_10bit: true,
            prefer_hdr_float: false,
            hdr_active: false,
            max_luminance_nits: 0,
            sdr_white_nits: 80,
        }),
        "bt2020" | "rec2020" => Some(DisplayColorInfo {
            protocol_supported: false,
            color_space: "bt2020".to_string(),
            color_space_code: 2,
            prefer_10bit: true,
            prefer_hdr_float: false,
            hdr_active: false,
            max_luminance_nits: 0,
            sdr_white_nits: 80,
        }),
        _ => None,
    }
}

fn wayland_socket_path() -> Option<PathBuf> {
    let display = std::env::var_os("WAYLAND_DISPLAY")?;
    if display.is_empty() {
        return None;
    }
    let path = PathBuf::from(&display);
    if path.is_absolute() {
        return Some(path);
    }
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    if runtime_dir.is_empty() {
        return None;
    }
    Some(PathBuf::from(runtime_dir).join(path))
}

#[derive(Default)]
struct OutputColorDesc {
    ready: bool,
    failed: bool,
    done: bool,
    primaries_named: Option<u32>,
    primaries_xy: Option<[i32; 8]>,
    tf_named: Option<u32>,
    max_lum: u32,
    ref_white: u32,
}

impl OutputColorDesc {
    fn to_info(&self) -> DisplayColorInfo {
        let is_hdr_tf = matches!(
            self.tf_named,
            Some(TF_ST2084_PQ | TF_HLG | TF_EXT_LINEAR | TF_EXT_SRGB)
        );
        let ref_white = if self.ref_white > 0 {
            self.ref_white
        } else if is_hdr_tf {
            203
        } else {
            80
        };
        let is_hdr_lum = self.max_lum > 250 && self.max_lum * 4 > ref_white * 5;
        let hdr_active = is_hdr_tf || is_hdr_lum;

        // Classify gamut from named primaries first, falling back to chromaticity coordinates.
        let gamut = match self.primaries_named {
            Some(PRIMARIES_BT2020) => 2,
            Some(PRIMARIES_DISPLAY_P3 | PRIMARIES_DCI_P3) => 1,
            Some(PRIMARIES_SRGB) => 0,
            _ => classify_xy_gamut(self.primaries_xy),
        };

        let (color_space, color_space_code) = match gamut {
            2 => ("bt2020", 2),
            1 => ("display_p3", 1),
            _ => ("srgb", 0),
        };
        let prefer_hdr_float = hdr_active;
        let prefer_10bit = hdr_active || gamut > 0;

        DisplayColorInfo {
            protocol_supported: true,
            color_space: color_space.to_string(),
            color_space_code,
            prefer_10bit,
            prefer_hdr_float,
            hdr_active,
            max_luminance_nits: self.max_lum,
            sdr_white_nits: ref_white,
        }
    }

    fn rank(&self) -> (u8, u8, u32) {
        let info = self.to_info();
        (u8::from(info.hdr_active), info.color_space_code as u8, info.max_luminance_nits)
    }
}

/// Classifies CIE xy primaries (`1_000_000` fixed-point units: `rx, ry, gx, gy, bx, by, wx, wy`)
/// into `2` (BT.2020), `1` (Display P3 / wide gamut), or `0` (sRGB).
fn classify_xy_gamut(xy: Option<[i32; 8]>) -> u8 {
    let Some([rx, _ry, gx, gy, _bx, _by, _wx, _wy]) = xy else {
        return 0;
    };
    // BT.2020 red x ≈ 0.708, green y ≈ 0.797.
    if rx >= 695_000 && gy >= 750_000 {
        return 2;
    }
    // Display P3 / DCI-P3 red x ≈ 0.680, green x ≈ 0.265, green y ≈ 0.690.
    if rx >= 665_000 && gx <= 285_000 && gy >= 650_000 {
        return 1;
    }
    0
}

fn push_msg(buf: &mut Vec<u8>, obj: u32, opcode: u16, payload: &[u8]) {
    let total = 8 + payload.len();
    debug_assert!(total <= u16::MAX as usize && total.is_multiple_of(4));
    buf.extend_from_slice(&obj.to_ne_bytes());
    let size_op = ((total as u32) << 16) | (opcode as u32);
    buf.extend_from_slice(&size_op.to_ne_bytes());
    buf.extend_from_slice(payload);
}

fn push_wl_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len_with_nul = (bytes.len() + 1) as u32;
    out.extend_from_slice(&len_with_nul.to_ne_bytes());
    out.extend_from_slice(bytes);
    out.push(0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn read_u32(slice: &[u8], off: usize) -> Option<u32> {
    let b: [u8; 4] = slice.get(off..off + 4)?.try_into().ok()?;
    Some(u32::from_ne_bytes(b))
}

fn read_i32(slice: &[u8], off: usize) -> Option<i32> {
    let b: [u8; 4] = slice.get(off..off + 4)?.try_into().ok()?;
    Some(i32::from_ne_bytes(b))
}

fn read_wl_string(slice: &[u8], off: usize) -> Option<(String, usize)> {
    let len = read_u32(slice, off)? as usize;
    if len == 0 || off + 4 + len > slice.len() {
        return None;
    }
    let raw = &slice[off + 4..off + 4 + len - 1];
    let s = String::from_utf8_lossy(raw).into_owned();
    let padded = (len + 3) & !3;
    Some((s, off + 4 + padded))
}

struct WireReader {
    buf: Vec<u8>,
    pos: usize,
    fds: VecDeque<OwnedFd>,
}

impl WireReader {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(4096),
            pos: 0,
            fds: VecDeque::new(),
        }
    }

    fn next_msg(&mut self, stream: &UnixStream, deadline: Instant) -> Option<(u32, u16, Vec<u8>)> {
        loop {
            if self.buf.len() - self.pos >= 8 {
                let obj = read_u32(&self.buf, self.pos)?;
                let size_op = read_u32(&self.buf, self.pos + 4)?;
                let size = (size_op >> 16) as usize;
                let opcode = (size_op & 0xFFFF) as u16;
                if size < 8 || !size.is_multiple_of(4) || size > 65_536 {
                    return None;
                }
                if self.buf.len() - self.pos >= size {
                    let payload = self.buf[self.pos + 8..self.pos + size].to_vec();
                    self.pos += size;
                    if self.pos >= 4096 && self.pos == self.buf.len() {
                        self.buf.clear();
                        self.pos = 0;
                    }
                    return Some((obj, opcode, payload));
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let _ = stream.set_read_timeout(Some(remaining));
            if !self.recv_more(stream.as_raw_fd()) {
                return None;
            }
        }
    }

    fn recv_more(&mut self, fd: RawFd) -> bool {
        let mut chunk = [0u8; 4096];
        let mut iov = libc::iovec {
            iov_base: chunk.as_mut_ptr().cast(),
            iov_len: chunk.len(),
        };
        // Control buffer large enough for up to 16 SCM_RIGHTS FDs.
        let mut cmsg_buf = [0u8; 256];
        let mut hdr: libc::msghdr = unsafe {
            // SAFETY: zero-initializing POSIX `msghdr` is standard and all fields are set below.
            std::mem::zeroed()
        };
        hdr.msg_iov = &mut iov;
        hdr.msg_iovlen = 1;
        hdr.msg_control = cmsg_buf.as_mut_ptr().cast();
        hdr.msg_controllen = cmsg_buf.len() as _;

        // SAFETY: `fd` is a valid connected Unix socket and `hdr` buffers live across `recvmsg`.
        let n = unsafe { libc::recvmsg(fd, &mut hdr, libc::MSG_CMSG_CLOEXEC) };
        if n <= 0 {
            return false;
        }
        self.buf.extend_from_slice(&chunk[..n as usize]);

        // SAFETY: iterate valid kernel-populated ancillary control headers and capture every
        // `SCM_RIGHTS` FD into `OwnedFd` so no descriptor can ever leak.
        unsafe {
            let mut cmsg = libc::CMSG_FIRSTHDR(&hdr);
            while !cmsg.is_null() {
                if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                    let data_ptr = libc::CMSG_DATA(cmsg);
                    let header_len = libc::CMSG_LEN(0) as usize;
                    let total_len = (*cmsg).cmsg_len as usize;
                    if total_len >= header_len {
                        let fd_count = (total_len - header_len) / std::mem::size_of::<RawFd>();
                        for idx in 0..fd_count {
                            let mut raw_fd: RawFd = -1;
                            std::ptr::copy_nonoverlapping(
                                data_ptr.add(idx * std::mem::size_of::<RawFd>()),
                                (&mut raw_fd as *mut RawFd).cast(),
                                std::mem::size_of::<RawFd>(),
                            );
                            if raw_fd >= 0 {
                                self.fds.push_back(OwnedFd::from_raw_fd(raw_fd));
                            }
                        }
                    }
                }
                cmsg = libc::CMSG_NXTHDR(&hdr, cmsg);
            }
        }
        true
    }
}

fn probe_stream(stream: &mut UnixStream) -> Option<DisplayColorInfo> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PROBE_TIMEOUT));

    // Roundtrip 1: enumerate Wayland globals (`wp_color_manager_v1` and `wl_output`).
    let registry_id = 2u32;
    let sync1_id = 3u32;
    let mut wire = Vec::with_capacity(256);
    push_msg(&mut wire, WL_DISPLAY_ID, WL_DISPLAY_GET_REGISTRY_OP, &registry_id.to_ne_bytes());
    push_msg(&mut wire, WL_DISPLAY_ID, WL_DISPLAY_SYNC_OP, &sync1_id.to_ne_bytes());
    stream.write_all(&wire).ok()?;

    let mut reader = WireReader::new();
    let mut color_manager_global: Option<(u32, u32)> = None;
    let mut output_globals: Vec<(u32, u32)> = Vec::new();
    while let Some((obj, op, payload)) = reader.next_msg(stream, deadline) {
        if obj == WL_DISPLAY_ID && op == WL_DISPLAY_ERROR_EVT {
            return None;
        }
        if obj == registry_id && op == WL_REGISTRY_GLOBAL_EVT {
            if let Some(name) = read_u32(&payload, 0)
                && let Some((iface, next_off)) = read_wl_string(&payload, 4)
                && let Some(ver) = read_u32(&payload, next_off)
            {
                if iface == WP_COLOR_MANAGER_IFACE {
                    color_manager_global = Some((name, ver.min(1)));
                } else if iface == WL_OUTPUT_IFACE && output_globals.len() < MAX_OUTPUTS {
                    output_globals.push((name, ver.min(4)));
                }
            }
        } else if obj == sync1_id && op == WL_CALLBACK_DONE_EVT {
            break;
        }
    }

    let (cm_name, cm_ver) = color_manager_global?;
    if output_globals.is_empty() || cm_ver == 0 {
        return Some(DisplayColorInfo {
            protocol_supported: true,
            ..DisplayColorInfo::default()
        });
    }

    // Roundtrip 2: bind `wp_color_manager_v1` (v1) and every `wl_output`, request each
    // output's `wp_color_management_output_v1` and current `wp_image_description_v1`.
    let cm_id = 4u32;
    let sync2_id = 5u32;
    let out_base = 10u32;
    let cm_out_base = 30u32;
    let desc_base = 50u32;
    let info_base = 70u32;

    wire.clear();
    let mut bind_payload = Vec::with_capacity(64);
    bind_payload.extend_from_slice(&cm_name.to_ne_bytes());
    push_wl_string(&mut bind_payload, WP_COLOR_MANAGER_IFACE);
    bind_payload.extend_from_slice(&1u32.to_ne_bytes());
    bind_payload.extend_from_slice(&cm_id.to_ne_bytes());
    push_msg(&mut wire, registry_id, WL_REGISTRY_BIND_OP, &bind_payload);

    for (idx, &(out_name, out_ver)) in output_globals.iter().enumerate() {
        let out_id = out_base + idx as u32;
        let cm_out_id = cm_out_base + idx as u32;
        let desc_id = desc_base + idx as u32;

        bind_payload.clear();
        bind_payload.extend_from_slice(&out_name.to_ne_bytes());
        push_wl_string(&mut bind_payload, WL_OUTPUT_IFACE);
        bind_payload.extend_from_slice(&out_ver.max(1).to_ne_bytes());
        bind_payload.extend_from_slice(&out_id.to_ne_bytes());
        push_msg(&mut wire, registry_id, WL_REGISTRY_BIND_OP, &bind_payload);

        let mut get_out = [0u8; 8];
        get_out[0..4].copy_from_slice(&cm_out_id.to_ne_bytes());
        get_out[4..8].copy_from_slice(&out_id.to_ne_bytes());
        push_msg(&mut wire, cm_id, WP_COLOR_MANAGER_GET_OUTPUT_OP, &get_out);

        push_msg(&mut wire, cm_out_id, WP_CM_OUTPUT_GET_IMAGE_DESC_OP, &desc_id.to_ne_bytes());
    }
    push_msg(&mut wire, WL_DISPLAY_ID, WL_DISPLAY_SYNC_OP, &sync2_id.to_ne_bytes());
    stream.write_all(&wire).ok()?;

    let mut descs: Vec<OutputColorDesc> = (0..output_globals.len())
        .map(|_| OutputColorDesc::default())
        .collect();
    let mut sync2_done = false;
    while !(sync2_done && descs.iter().all(|d| d.ready || d.failed)) {
        let Some((obj, op, _payload)) = reader.next_msg(stream, deadline) else {
            break;
        };
        if obj == WL_DISPLAY_ID && op == WL_DISPLAY_ERROR_EVT {
            return None;
        }
        if obj == sync2_id && op == WL_CALLBACK_DONE_EVT {
            sync2_done = true;
            continue;
        }
        if obj >= desc_base && (obj - desc_base) < descs.len() as u32 {
            let d = &mut descs[(obj - desc_base) as usize];
            if op == WP_IMG_DESC_READY_EVT {
                d.ready = true;
            } else if op == WP_IMG_DESC_FAILED_EVT {
                d.failed = true;
            }
        }
    }

    // Roundtrip 3: request `wp_image_description_info_v1` for every ready description.
    let sync3_id = 6u32;
    wire.clear();
    let mut queried = 0usize;
    for (idx, d) in descs.iter().enumerate() {
        if d.ready {
            let desc_id = desc_base + idx as u32;
            let info_id = info_base + idx as u32;
            push_msg(&mut wire, desc_id, WP_IMG_DESC_GET_INFO_OP, &info_id.to_ne_bytes());
            queried += 1;
        }
    }
    if queried > 0 {
        push_msg(&mut wire, WL_DISPLAY_ID, WL_DISPLAY_SYNC_OP, &sync3_id.to_ne_bytes());
        stream.write_all(&wire).ok()?;

        let mut sync3_done = false;
        while !(sync3_done && descs.iter().all(|d| !d.ready || d.done)) {
            let Some((obj, op, payload)) = reader.next_msg(stream, deadline) else {
                break;
            };
            if obj == WL_DISPLAY_ID && op == WL_DISPLAY_ERROR_EVT {
                break;
            }
            if obj == sync3_id && op == WL_CALLBACK_DONE_EVT {
                sync3_done = true;
                continue;
            }
            if obj >= info_base && (obj - info_base) < descs.len() as u32 {
                let d = &mut descs[(obj - info_base) as usize];
                match op {
                    WP_IMG_INFO_DONE_EVT => d.done = true,
                    WP_IMG_INFO_ICC_FILE_EVT => {
                        // Drop the attached ICC file descriptor immediately so we never leak FDs.
                        let _ = reader.fds.pop_front();
                    }
                    WP_IMG_INFO_PRIMARIES_EVT => {
                        if payload.len() >= 32 {
                            let mut xy = [0i32; 8];
                            for (k, slot) in xy.iter_mut().enumerate() {
                                *slot = read_i32(&payload, k * 4).unwrap_or(0);
                            }
                            d.primaries_xy = Some(xy);
                        }
                    }
                    WP_IMG_INFO_PRIMARIES_NAMED_EVT => {
                        d.primaries_named = read_u32(&payload, 0);
                    }
                    WP_IMG_INFO_TF_NAMED_EVT => {
                        d.tf_named = read_u32(&payload, 0);
                    }
                    WP_IMG_INFO_LUMINANCES_EVT => {
                        if let (Some(max_l), Some(ref_w)) = (read_u32(&payload, 4), read_u32(&payload, 8)) {
                            d.max_lum = d.max_lum.max(max_l);
                            if ref_w > 0 {
                                d.ref_white = ref_w;
                            }
                        }
                    }
                    WP_IMG_INFO_TARGET_LUMINANCE_EVT => {
                        if let Some(max_l) = read_u32(&payload, 4) {
                            d.max_lum = d.max_lum.max(max_l);
                        }
                    }
                    WP_IMG_INFO_TARGET_MAX_CLL_EVT => {
                        if let Some(max_cll) = read_u32(&payload, 0) {
                            d.max_lum = d.max_lum.max(max_cll);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Best-effort protocol cleanup before closing the probe socket.
    wire.clear();
    for idx in 0..output_globals.len() {
        push_msg(&mut wire, desc_base + idx as u32, WP_IMG_DESC_DESTROY_OP, &[]);
        push_msg(&mut wire, cm_out_base + idx as u32, WP_CM_OUTPUT_DESTROY_OP, &[]);
    }
    push_msg(&mut wire, cm_id, WP_COLOR_MANAGER_DESTROY_OP, &[]);
    let _ = stream.write_all(&wire);

    let best = descs
        .iter()
        .filter(|d| d.done)
        .max_by_key(|d| d.rank());
    match best {
        Some(desc) => Some(desc.to_info()),
        None => Some(DisplayColorInfo {
            protocol_supported: true,
            ..DisplayColorInfo::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::thread;

    fn send_with_fd(sock: &UnixStream, data: &[u8], fd: RawFd) {
        let mut iov = libc::iovec {
            iov_base: data.as_ptr().cast_mut().cast(),
            iov_len: data.len(),
        };
        let mut cmsg_buf = [0u8; 64];
        let mut hdr: libc::msghdr = unsafe {
            // SAFETY: zero-initialized POSIX `msghdr` populated below.
            std::mem::zeroed()
        };
        hdr.msg_iov = &mut iov;
        hdr.msg_iovlen = 1;
        hdr.msg_control = cmsg_buf.as_mut_ptr().cast();
        // SAFETY: `CMSG_SPACE` for one `RawFd` fits within our 64-byte stack buffer.
        hdr.msg_controllen = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as _ };
        // SAFETY: `CMSG_FIRSTHDR` returns a valid aligned pointer into `cmsg_buf`.
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&hdr);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
            std::ptr::copy_nonoverlapping(
                (&fd as *const RawFd).cast::<u8>(),
                libc::CMSG_DATA(cmsg),
                std::mem::size_of::<RawFd>(),
            );
            let sent = libc::sendmsg(sock.as_raw_fd(), &hdr, 0);
            assert_eq!(sent as usize, data.len());
        }
    }

    fn read_client_msgs(sock: &mut UnixStream, min_bytes: usize) -> Vec<(u32, u16, Vec<u8>)> {
        let mut raw = Vec::new();
        let mut tmp = [0u8; 512];
        while raw.len() < min_bytes {
            let n = sock.read(&mut tmp).unwrap();
            assert!(n > 0);
            raw.extend_from_slice(&tmp[..n]);
        }
        let mut msgs = Vec::new();
        let mut pos = 0;
        while pos + 8 <= raw.len() {
            let obj = read_u32(&raw, pos).unwrap();
            let size_op = read_u32(&raw, pos + 4).unwrap();
            let size = (size_op >> 16) as usize;
            let op = (size_op & 0xFFFF) as u16;
            msgs.push((obj, op, raw[pos + 8..pos + size].to_vec()));
            pos += size;
        }
        msgs
    }

    fn push_global(out: &mut Vec<u8>, registry_id: u32, name: u32, iface: &str, ver: u32) {
        let mut p = Vec::new();
        p.extend_from_slice(&name.to_ne_bytes());
        push_wl_string(&mut p, iface);
        p.extend_from_slice(&ver.to_ne_bytes());
        push_msg(out, registry_id, WL_REGISTRY_GLOBAL_EVT, &p);
    }

    #[test]
    fn mock_wayland_compositor_reports_hdr_bt2020_pq_and_closes_icc_fd() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            // Roundtrip 1: get_registry + sync
            let msgs1 = read_client_msgs(&mut server, 24);
            let reg_id = read_u32(&msgs1[0].2, 0).unwrap();
            let sync1 = read_u32(&msgs1[1].2, 0).unwrap();
            let mut reply = Vec::new();
            push_global(&mut reply, reg_id, 10, WP_COLOR_MANAGER_IFACE, 1);
            push_global(&mut reply, reg_id, 11, WL_OUTPUT_IFACE, 4);
            push_msg(&mut reply, sync1, WL_CALLBACK_DONE_EVT, &1u32.to_ne_bytes());
            server.write_all(&reply).unwrap();

            // Roundtrip 2: bind cm + bind output + get_output + get_image_description + sync
            let msgs2 = read_client_msgs(&mut server, 64);
            let desc_id = read_u32(&msgs2[3].2, 0).unwrap();
            let sync2 = read_u32(&msgs2[4].2, 0).unwrap();
            reply.clear();
            push_msg(&mut reply, desc_id, WP_IMG_DESC_READY_EVT, &42u32.to_ne_bytes());
            push_msg(&mut reply, sync2, WL_CALLBACK_DONE_EVT, &2u32.to_ne_bytes());
            server.write_all(&reply).unwrap();

            // Roundtrip 3: get_information + sync
            let msgs3 = read_client_msgs(&mut server, 24);
            let info_id = read_u32(&msgs3[0].2, 0).unwrap();
            let sync3 = read_u32(&msgs3[1].2, 0).unwrap();

            // Send an `icc_file` event with a real SCM_RIGHTS eventfd to prove FD cleanup.
            // SAFETY: standalone `eventfd` closed by `OwnedFd` drop right after `sendmsg`.
            let icc_fd = unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC)) };
            let mut icc_msg = Vec::new();
            push_msg(&mut icc_msg, info_id, WP_IMG_INFO_ICC_FILE_EVT, &1024u32.to_ne_bytes());
            send_with_fd(&server, &icc_msg, icc_fd.as_raw_fd());

            reply.clear();
            push_msg(&mut reply, info_id, WP_IMG_INFO_PRIMARIES_NAMED_EVT, &PRIMARIES_BT2020.to_ne_bytes());
            push_msg(&mut reply, info_id, WP_IMG_INFO_TF_NAMED_EVT, &TF_ST2084_PQ.to_ne_bytes());
            let mut lum = [0u8; 12];
            lum[0..4].copy_from_slice(&500u32.to_ne_bytes());
            lum[4..8].copy_from_slice(&1000u32.to_ne_bytes());
            lum[8..12].copy_from_slice(&203u32.to_ne_bytes());
            push_msg(&mut reply, info_id, WP_IMG_INFO_LUMINANCES_EVT, &lum);
            push_msg(&mut reply, info_id, WP_IMG_INFO_DONE_EVT, &[]);
            push_msg(&mut reply, sync3, WL_CALLBACK_DONE_EVT, &3u32.to_ne_bytes());
            server.write_all(&reply).unwrap();
        });

        let info = probe_stream(&mut client).expect("probe should succeed");
        handle.join().unwrap();

        assert!(info.protocol_supported);
        assert!(info.hdr_active);
        assert!(info.prefer_hdr_float);
        assert!(info.prefer_10bit);
        assert_eq!(info.color_space, "bt2020");
        assert_eq!(info.color_space_code, 2);
        assert_eq!(info.max_luminance_nits, 1000);
        assert_eq!(info.sdr_white_nits, 203);
    }

    #[test]
    fn mock_wayland_compositor_reports_display_p3_10bit_sdr() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            let msgs1 = read_client_msgs(&mut server, 24);
            let reg_id = read_u32(&msgs1[0].2, 0).unwrap();
            let sync1 = read_u32(&msgs1[1].2, 0).unwrap();
            let mut reply = Vec::new();
            push_global(&mut reply, reg_id, 1, WP_COLOR_MANAGER_IFACE, 1);
            push_global(&mut reply, reg_id, 2, WL_OUTPUT_IFACE, 3);
            push_msg(&mut reply, sync1, WL_CALLBACK_DONE_EVT, &1u32.to_ne_bytes());
            server.write_all(&reply).unwrap();

            let msgs2 = read_client_msgs(&mut server, 64);
            let desc_id = read_u32(&msgs2[3].2, 0).unwrap();
            let sync2 = read_u32(&msgs2[4].2, 0).unwrap();
            reply.clear();
            push_msg(&mut reply, desc_id, WP_IMG_DESC_READY_EVT, &7u32.to_ne_bytes());
            push_msg(&mut reply, sync2, WL_CALLBACK_DONE_EVT, &2u32.to_ne_bytes());
            server.write_all(&reply).unwrap();

            let msgs3 = read_client_msgs(&mut server, 24);
            let info_id = read_u32(&msgs3[0].2, 0).unwrap();
            let sync3 = read_u32(&msgs3[1].2, 0).unwrap();
            reply.clear();
            push_msg(&mut reply, info_id, WP_IMG_INFO_PRIMARIES_NAMED_EVT, &PRIMARIES_DISPLAY_P3.to_ne_bytes());
            push_msg(&mut reply, info_id, WP_IMG_INFO_TF_NAMED_EVT, &9u32.to_ne_bytes());
            push_msg(&mut reply, info_id, WP_IMG_INFO_DONE_EVT, &[]);
            push_msg(&mut reply, sync3, WL_CALLBACK_DONE_EVT, &3u32.to_ne_bytes());
            server.write_all(&reply).unwrap();
        });

        let info = probe_stream(&mut client).expect("probe should succeed");
        handle.join().unwrap();

        assert!(info.protocol_supported);
        assert!(!info.hdr_active);
        assert!(!info.prefer_hdr_float);
        assert!(info.prefer_10bit);
        assert_eq!(info.color_space, "display_p3");
        assert_eq!(info.color_space_code, 1);
    }

    #[test]
    fn mock_wayland_compositor_without_color_management_falls_back_to_srgb() {
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            let msgs1 = read_client_msgs(&mut server, 24);
            let reg_id = read_u32(&msgs1[0].2, 0).unwrap();
            let sync1 = read_u32(&msgs1[1].2, 0).unwrap();
            let mut reply = Vec::new();
            push_global(&mut reply, reg_id, 1, WL_OUTPUT_IFACE, 4);
            push_msg(&mut reply, sync1, WL_CALLBACK_DONE_EVT, &1u32.to_ne_bytes());
            server.write_all(&reply).unwrap();
        });

        assert!(probe_stream(&mut client).is_none());
        handle.join().unwrap();
    }

    #[test]
    fn hdr_env_overrides_parse_cleanly() {
        let scrgb = parse_hdr_env_override("scrgb").unwrap();
        assert!(scrgb.hdr_active && scrgb.prefer_hdr_float && scrgb.prefer_10bit);
        assert_eq!(scrgb.color_space, "display_p3");

        let hdr10 = parse_hdr_env_override("HDR10").unwrap();
        assert!(hdr10.hdr_active && !hdr10.prefer_hdr_float && hdr10.prefer_10bit);
        assert_eq!(hdr10.color_space, "bt2100_pq");
        assert_eq!(hdr10.color_space_code, 3);

        let p3 = parse_hdr_env_override("display_p3").unwrap();
        assert!(!p3.hdr_active && !p3.prefer_hdr_float && p3.prefer_10bit);
        assert_eq!(p3.color_space_code, 1);

        let off = parse_hdr_env_override("off").unwrap();
        assert!(!off.hdr_active && !off.prefer_10bit);
        assert_eq!(off.color_space, "srgb");
    }
}
