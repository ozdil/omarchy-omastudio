// Zero-disk shared-memory preview ring (`memfd_create` + `SCM_RIGHTS` + seqlock).
//
// Shared between `omastudio-engine` (producer) and `omastudio-app` (consumer) so both
// binaries agree on the exact wire protocol, slot layout, and memory ordering without
// linking the engine's heavy dependencies into the Qt frontend.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

/// Fixed-size atomic header stored at byte offset 0 of every `memfd` slot.
pub const SLOT_HEADER_BYTES: usize = 64;
/// Maximum accepted single-frame payload bytes (fits 3840x3840x8 = 112.5 MiB with headroom).
pub const MAX_FRAME_BYTES: usize = 128 << 20;
/// Maximum accepted single image dimension.
pub const MAX_DIMENSION: u32 = 16_384;

/// Binary magic for the `SCM_RIGHTS` preview control packets.
pub const RING_MAGIC: [u8; 4] = *b"OMFD";
pub const RING_VERSION: u32 = 1;
pub const MSG_KIND_MEMFD_RING_INIT: u32 = 1;
pub const MSG_KIND_DMABUF_FRAME: u32 = 2;
/// 4-byte acknowledgement sent by `omastudio-app` after mapping slots or parking a DMA-BUF frame.
pub const RING_ACK_OK: [u8; 4] = *b"OKAY";

/// Vulkan `VK_FORMAT_R16G16B16A16_SFLOAT` (core WebGPU `Rgba16Float` storage format).
#[allow(dead_code)]
pub const VK_FORMAT_R16G16B16A16_SFLOAT: u32 = 97;
/// FourCC `AB4H` (`DRM_FORMAT_ABGR16161616F`), matching `VK_FORMAT_R16G16B16A16_SFLOAT`.
#[allow(dead_code)]
pub const DRM_FORMAT_ABGR16161616F: u32 = u32::from_le_bytes(*b"AB4H");
/// `DRM_FORMAT_MOD_INVALID` when a driver does not advertise DRM format modifiers.
#[allow(dead_code)]
pub const DRM_FORMAT_MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

/// Environment variable carrying the inherited Unix `SOCK_SEQPACKET` FD into `omastudio-engine daemon`.
pub const ENV_PREVIEW_FD: &str = "OMASTUDIO_PREVIEW_FD";

/// Number of double-buffered slots per interactive view kind (`preview`, `reference`, `detail`, `loupe`).
pub const SLOTS_PER_KIND: usize = 2;
pub const TOTAL_SLOTS: usize = 4 * SLOTS_PER_KIND;

/// Sparse virtual capacities per slot. Linux `memfd_create` + `ftruncate` only allocates
/// physical tmpfs pages for bytes actually written; untouched tail pages consume zero RSS.
pub const PREVIEW_SLOT_CAPACITY: usize = 128 << 20;
pub const REFERENCE_SLOT_CAPACITY: usize = 128 << 20;
pub const DETAIL_SLOT_CAPACITY: usize = 64 << 20;
pub const LOUPE_SLOT_CAPACITY: usize = 8 << 20;

const SLOT_CAPACITIES: [usize; TOTAL_SLOTS] = [
    PREVIEW_SLOT_CAPACITY,
    PREVIEW_SLOT_CAPACITY,
    REFERENCE_SLOT_CAPACITY,
    REFERENCE_SLOT_CAPACITY,
    DETAIL_SLOT_CAPACITY,
    DETAIL_SLOT_CAPACITY,
    LOUPE_SLOT_CAPACITY,
    LOUPE_SLOT_CAPACITY,
];

/// Interactive image stream served via `image://omastudio/<kind>/<slot>/<gen>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Preview = 0,
    Reference = 1,
    Detail = 2,
    Loupe = 3,
}

impl SlotKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Reference => "reference",
            Self::Detail => "detail",
            Self::Loupe => "loupe",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "preview" => Some(Self::Preview),
            "reference" => Some(Self::Reference),
            "detail" => Some(Self::Detail),
            "loupe" => Some(Self::Loupe),
            _ => None,
        }
    }

    pub const fn base_slot(self) -> usize {
        (self as usize) * SLOTS_PER_KIND
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Pixel formats supported across the `memfd` boundary and Qt `QQuickImageProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PixelFormat {
    Rgb888 = 0,
    Rgba8888 = 1,
    Rgba16 = 2,
    Rgba16F = 3,
}

#[allow(dead_code)]
impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Rgb888 => 3,
            Self::Rgba8888 => 4,
            Self::Rgba16 | Self::Rgba16F => 8,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rgb888 => "rgb888",
            Self::Rgba8888 => "rgba8888",
            Self::Rgba16 => "rgba16",
            Self::Rgba16F => "rgba16f",
        }
    }

    pub const fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Rgb888),
            1 => Some(Self::Rgba8888),
            2 => Some(Self::Rgba16),
            3 => Some(Self::Rgba16F),
            _ => None,
        }
    }
}

/// Colour space tags translated to Qt `QColorSpace` by `OmaPreviewImageProvider`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ColorSpaceTag {
    Srgb = 0,
    DisplayP3 = 1,
    Bt2020 = 2,
    Bt2100Pq = 3,
    Bt2100Hlg = 4,
}

#[allow(dead_code)]
impl ColorSpaceTag {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Srgb => "srgb",
            Self::DisplayP3 => "display_p3",
            Self::Bt2020 => "bt2020",
            Self::Bt2100Pq => "bt2100_pq",
            Self::Bt2100Hlg => "bt2100_hlg",
        }
    }

    pub const fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Srgb),
            1 => Some(Self::DisplayP3),
            2 => Some(Self::Bt2020),
            3 => Some(Self::Bt2100Pq),
            4 => Some(Self::Bt2100Hlg),
            _ => None,
        }
    }
}

/// Seqlock-protected metadata stored at byte offset 0 of every `memfd` slot.
#[repr(C, align(64))]
struct SlotHeader {
    seq: AtomicU64,
    generation: AtomicU64,
    width: AtomicU32,
    height: AtomicU32,
    stride: AtomicU32,
    format: AtomicU32,
    color_space: AtomicU32,
    _reserved: [AtomicU32; 7],
}

const _: () = assert!(std::mem::size_of::<SlotHeader>() == SLOT_HEADER_BYTES);

/// Wire payload accompanying the `SCM_RIGHTS` FD array during ring initialization.
const INIT_MSG_BYTES: usize = 16 + TOTAL_SLOTS * 8;

fn encode_init_msg(capacities: &[usize; TOTAL_SLOTS]) -> [u8; INIT_MSG_BYTES] {
    let mut buf = [0u8; INIT_MSG_BYTES];
    buf[0..4].copy_from_slice(&RING_MAGIC);
    buf[4..8].copy_from_slice(&RING_VERSION.to_le_bytes());
    buf[8..12].copy_from_slice(&MSG_KIND_MEMFD_RING_INIT.to_le_bytes());
    buf[12..16].copy_from_slice(&(TOTAL_SLOTS as u32).to_le_bytes());
    for (i, &cap) in capacities.iter().enumerate() {
        let off = 16 + i * 8;
        buf[off..off + 8].copy_from_slice(&(cap as u64).to_le_bytes());
    }
    buf
}

fn decode_init_msg(buf: &[u8]) -> Option<[usize; TOTAL_SLOTS]> {
    if buf.len() != INIT_MSG_BYTES {
        return None;
    }
    if buf[0..4] != RING_MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(buf[4..8].try_into().ok()?);
    let kind = u32::from_le_bytes(buf[8..12].try_into().ok()?);
    let count = u32::from_le_bytes(buf[12..16].try_into().ok()?);
    if version != RING_VERSION || kind != MSG_KIND_MEMFD_RING_INIT || count as usize != TOTAL_SLOTS {
        return None;
    }
    let mut caps = [0usize; TOTAL_SLOTS];
    for (i, slot) in caps.iter_mut().enumerate() {
        let off = 16 + i * 8;
        let cap_u64 = u64::from_le_bytes(buf[off..off + 8].try_into().ok()?);
        let cap = usize::try_from(cap_u64).ok()?;
        if !(SLOT_HEADER_BYTES..=MAX_FRAME_BYTES + SLOT_HEADER_BYTES).contains(&cap) {
            return None;
        }
        *slot = cap;
    }
    Some(caps)
}

/// Interactive preview transport selected via `OMASTUDIO_PREVIEW_MODE` (`auto|dma-buf|memfd|shm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PreviewMode {
    Auto,
    DmaBuf,
    Memfd,
    File,
}

#[allow(dead_code)]
pub fn parse_preview_mode(raw: Option<&str>) -> PreviewMode {
    match raw.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("dma-buf" | "dmabuf" | "vulkan") => PreviewMode::DmaBuf,
        Some("memfd") => PreviewMode::Memfd,
        Some("shm" | "ppm" | "file") => PreviewMode::File,
        _ => PreviewMode::Auto,
    }
}

#[allow(dead_code)]
pub fn preview_mode_from_env() -> PreviewMode {
    let raw = std::env::var("OMASTUDIO_PREVIEW_MODE").ok();
    parse_preview_mode(raw.as_deref())
}

/// Returns whether the user explicitly disabled `memfd` previews via `OMASTUDIO_PREVIEW_MODE`.
#[allow(dead_code)]
pub fn memfd_disabled_by_env() -> bool {
    preview_mode_from_env() == PreviewMode::File
}

/// Returns whether GPU-to-GPU DMA-BUF previews are permitted by environment overrides.
#[allow(dead_code)]
pub fn dmabuf_enabled_by_env() -> bool {
    if std::env::var("QT_QUICK_BACKEND")
        .ok()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("software"))
    {
        return false;
    }
    if std::env::var("QSG_RHI_BACKEND")
        .ok()
        .is_some_and(|v| !v.trim().is_empty() && !v.trim().eq_ignore_ascii_case("vulkan"))
    {
        return false;
    }
    matches!(preview_mode_from_env(), PreviewMode::Auto | PreviewMode::DmaBuf)
}

/// Creates a connected Unix `SOCK_SEQPACKET | SOCK_CLOEXEC` socketpair for FD passing.
#[allow(dead_code)]
pub fn create_preview_socketpair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1 as libc::c_int; 2];
    // SAFETY: `fds` points to a valid 2-element `c_int` array; kernel writes both descriptors on 0.
    let rc = unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `socketpair` succeeded and returned two newly created unique open file descriptors.
    let a = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: `socketpair` succeeded and returned two newly created unique open file descriptors.
    let b = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    Ok((a, b))
}

/// RAII wrapper around a `MAP_SHARED` `memfd` mapping.
struct MmapSlot {
    _fd: OwnedFd,
    ptr: NonNull<u8>,
    len: usize,
}

// SAFETY: `MmapSlot` owns a valid process-wide `mmap` region and its underlying `OwnedFd`.
// Producer/consumer access is synchronized via the atomic seqlock header in the mapping.
unsafe impl Send for MmapSlot {}
// SAFETY: Shared read-only references to `MmapSlot` only perform atomic header loads and
// seqlock-bracketed copies out of the `PROT_READ` mapping.
unsafe impl Sync for MmapSlot {}

impl MmapSlot {
    fn map(fd: OwnedFd, len: usize, writable: bool) -> io::Result<Self> {
        let prot = if writable {
            libc::PROT_READ | libc::PROT_WRITE
        } else {
            libc::PROT_READ
        };
        // SAFETY: `fd` is a valid open file descriptor of at least `len` bytes with `F_SEAL_SHRINK`;
        // `mmap` returns `MAP_FAILED` on error which we check before constructing `NonNull`.
        let raw = unsafe { libc::mmap(std::ptr::null_mut(), len, prot, libc::MAP_SHARED, fd.as_raw_fd(), 0) };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        let ptr = NonNull::new(raw.cast::<u8>())
            .ok_or_else(|| io::Error::other("mmap returned null pointer"))?;
        Ok(Self { _fd: fd, ptr, len })
    }

    fn header(&self) -> &SlotHeader {
        // SAFETY: `self.ptr` is page-aligned (>= 64-byte alignment) and `self.len >= SLOT_HEADER_BYTES`,
        // backed by a sealed `memfd` valid for the lifetime of `self`.
        unsafe { &*self.ptr.as_ptr().cast::<SlotHeader>() }
    }

    fn payload_ptr(&self) -> *const u8 {
        // SAFETY: `self.len >= SLOT_HEADER_BYTES`, so offsetting by `SLOT_HEADER_BYTES` stays in-bounds.
        unsafe { self.ptr.as_ptr().add(SLOT_HEADER_BYTES) }
    }

    fn payload_mut_ptr(&mut self) -> *mut u8 {
        // SAFETY: `self.len >= SLOT_HEADER_BYTES` and `MmapSlot` is uniquely borrowed by the writer.
        unsafe { self.ptr.as_ptr().add(SLOT_HEADER_BYTES) }
    }

    fn payload_capacity(&self) -> usize {
        self.len.saturating_sub(SLOT_HEADER_BYTES)
    }

    fn raw_fd(&self) -> RawFd {
        self._fd.as_raw_fd()
    }
}

impl Drop for MmapSlot {
    fn drop(&mut self) {
        // SAFETY: `self.ptr` and `self.len` came from a successful `libc::mmap` in `MmapSlot::map`.
        unsafe {
            libc::munmap(self.ptr.as_ptr().cast(), self.len);
        }
    }
}

/// Metadata returned when `omastudio-engine` publishes a frame into the `memfd` ring.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PublishedFrame {
    pub url: String,
    pub slot: usize,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub color_space: ColorSpaceTag,
}

/// Producer-side `memfd` ring owned by `omastudio-engine daemon`.
#[allow(dead_code)]
pub struct PreviewRingWriter {
    slots: Vec<MmapSlot>,
    next_slot: [usize; 4],
    generation: u64,
}

#[allow(dead_code, clippy::too_many_arguments)]
impl PreviewRingWriter {
    /// Creates all 8 anonymous `memfd` slots, seals them against shrinking, and maps them RW.
    pub fn create() -> io::Result<Self> {
        let mut slots = Vec::with_capacity(TOTAL_SLOTS);
        for (idx, &cap) in SLOT_CAPACITIES.iter().enumerate() {
            let name = CString::new(format!("omastudio-preview-{idx}")).expect("no interior NUL");
            // SAFETY: `name` is a valid NUL-terminated C string; Linux `memfd_create` returns a new FD or -1.
            let raw_fd = unsafe {
                libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING)
            };
            if raw_fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `raw_fd >= 0` is a newly created file descriptor owned by this scope.
            let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
            // SAFETY: `fd` is a valid open `memfd`; `cap` fits in `off_t`.
            if unsafe { libc::ftruncate(fd.as_raw_fd(), cap as libc::off_t) } != 0 {
                return Err(io::Error::last_os_error());
            }
            // Prevent shrinking so the receiver can never hit SIGBUS on mmap reads.
            // SAFETY: `fd` was created with `MFD_ALLOW_SEALING`; `F_ADD_SEALS` takes `c_int` flags.
            if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_ADD_SEALS, libc::F_SEAL_SHRINK) } != 0 {
                return Err(io::Error::last_os_error());
            }
            let slot = MmapSlot::map(fd, cap, true)?;
            slot.header().seq.store(0, Ordering::Relaxed);
            slot.header().generation.store(0, Ordering::Relaxed);
            slots.push(slot);
        }
        Ok(Self {
            slots,
            next_slot: [0; 4],
            generation: 0,
        })
    }

    /// Transfers all 8 `memfd` descriptors over `sock_fd` via `SCM_RIGHTS` and waits up to
    /// `timeout_ms` for `omastudio-app` to map them and reply with `RING_ACK_OK`.
    pub fn send_and_wait_ack(&self, sock_fd: RawFd, timeout_ms: i32) -> io::Result<()> {
        let payload = encode_init_msg(&SLOT_CAPACITIES);
        let fds: [RawFd; TOTAL_SLOTS] = std::array::from_fn(|i| self.slots[i].raw_fd());
        send_fds(sock_fd, &payload, &fds)?;

        let mut pfd = libc::pollfd {
            fd: sock_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            // SAFETY: `pfd` points to one valid `pollfd` struct.
            let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if rc < 0 {
                let err = io::Error::last_os_error();
                if err.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(err);
            }
            if rc == 0 {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "timed out waiting for preview ring ack"));
            }
            break;
        }

        let mut ack = [0u8; 4];
        // SAFETY: `ack` is a valid 4-byte writable buffer; `sock_fd` is an open Unix socket.
        let n = unsafe { libc::recv(sock_fd, ack.as_mut_ptr().cast(), ack.len(), 0) };
        if n != 4 || ack != RING_ACK_OK {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid preview ring ack from host app",
            ));
        }
        Ok(())
    }

    /// Packs tightly-packed `RGB888` (`width * height * 3`) straight into the next `RGBA8888`
    /// `memfd` slot without intermediate heap allocation or `/dev/shm` I/O.
    pub fn write_rgb8_as_rgba8(
        &mut self,
        kind: SlotKind,
        rgb: &[u8],
        width: u32,
        height: u32,
        color_space: ColorSpaceTag,
    ) -> Option<PublishedFrame> {
        let pixels = (width as usize).checked_mul(height as usize)?;
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return None;
        }
        if rgb.len() != pixels.checked_mul(3)? {
            return None;
        }
        let stride = width.checked_mul(4)?;
        let out_bytes = (stride as usize).checked_mul(height as usize)?;
        let local_slot = self.next_slot[kind.index()] % SLOTS_PER_KIND;
        let abs_slot = kind.base_slot() + local_slot;
        let slot = self.slots.get_mut(abs_slot)?;
        if out_bytes > slot.payload_capacity() || out_bytes > MAX_FRAME_BYTES {
            return None;
        }

        self.next_slot[kind.index()] = (local_slot + 1) % SLOTS_PER_KIND;
        self.generation = self.generation.wrapping_add(1).max(1);
        let generation = self.generation;

        let prev_seq = slot.header().seq.load(Ordering::Relaxed);
        let writing_seq = (prev_seq & !1u64).wrapping_add(1);
        slot.header().seq.store(writing_seq, Ordering::Release);
        fence(Ordering::Release);

        slot.header().generation.store(generation, Ordering::Relaxed);
        slot.header().width.store(width, Ordering::Relaxed);
        slot.header().height.store(height, Ordering::Relaxed);
        slot.header().stride.store(stride, Ordering::Relaxed);
        slot.header().format.store(PixelFormat::Rgba8888 as u32, Ordering::Relaxed);
        slot.header().color_space.store(color_space as u32, Ordering::Relaxed);

        let dst_ptr = slot.payload_mut_ptr();
        // SAFETY: `out_bytes <= slot.payload_capacity()`, `dst_ptr` is uniquely owned by the writer,
        // and we write exactly `pixels * 4 == out_bytes` bytes while `seq` is odd.
        let dst = unsafe { std::slice::from_raw_parts_mut(dst_ptr, out_bytes) };
        let (src_chunks, _) = rgb.as_chunks::<3>();
        let (dst_chunks, _) = dst.as_chunks_mut::<4>();
        for (src_px, dst_px) in src_chunks.iter().zip(dst_chunks.iter_mut()) {
            *dst_px = [src_px[0], src_px[1], src_px[2], 255];
        }

        fence(Ordering::Release);
        slot.header().seq.store(writing_seq.wrapping_add(1), Ordering::Release);

        Some(PublishedFrame {
            url: format!("image://omastudio/{}/{}/{}", kind.as_str(), local_slot, generation),
            slot: local_slot,
            generation,
            width,
            height,
            stride,
            format: PixelFormat::Rgba8888,
            color_space,
        })
    }

    /// Writes an already formatted pixel buffer (`rgb888`, `rgba8888`, `rgba16`, or `rgba16f`)
    /// directly into the next `memfd` slot for `kind`.
    pub fn write_raw(
        &mut self,
        kind: SlotKind,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        format: PixelFormat,
        color_space: ColorSpaceTag,
    ) -> Option<PublishedFrame> {
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return None;
        }
        let min_stride = width.checked_mul(format.bytes_per_pixel())?;
        if stride < min_stride {
            return None;
        }
        let out_bytes = (stride as usize).checked_mul(height as usize)?;
        if data.len() != out_bytes || out_bytes > MAX_FRAME_BYTES {
            return None;
        }
        let local_slot = self.next_slot[kind.index()] % SLOTS_PER_KIND;
        let abs_slot = kind.base_slot() + local_slot;
        let slot = self.slots.get_mut(abs_slot)?;
        if out_bytes > slot.payload_capacity() {
            return None;
        }

        self.next_slot[kind.index()] = (local_slot + 1) % SLOTS_PER_KIND;
        self.generation = self.generation.wrapping_add(1).max(1);
        let generation = self.generation;

        let prev_seq = slot.header().seq.load(Ordering::Relaxed);
        let writing_seq = (prev_seq & !1u64).wrapping_add(1);
        slot.header().seq.store(writing_seq, Ordering::Release);
        fence(Ordering::Release);

        slot.header().generation.store(generation, Ordering::Relaxed);
        slot.header().width.store(width, Ordering::Relaxed);
        slot.header().height.store(height, Ordering::Relaxed);
        slot.header().stride.store(stride, Ordering::Relaxed);
        slot.header().format.store(format as u32, Ordering::Relaxed);
        slot.header().color_space.store(color_space as u32, Ordering::Relaxed);

        let dst_ptr = slot.payload_mut_ptr();
        // SAFETY: `out_bytes == data.len() <= slot.payload_capacity()` and regions do not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), dst_ptr, out_bytes);
        }

        fence(Ordering::Release);
        slot.header().seq.store(writing_seq.wrapping_add(1), Ordering::Release);

        Some(PublishedFrame {
            url: format!("image://omastudio/{}/{}/{}", kind.as_str(), local_slot, generation),
            slot: local_slot,
            generation,
            width,
            height,
            stride,
            format,
            color_space,
        })
    }
}

/// Validates an inherited `OMASTUDIO_PREVIEW_FD` in `omastudio-engine daemon` and sets `FD_CLOEXEC`
/// so grandchild processes never inherit the IPC socket.
#[allow(dead_code)]
pub fn take_inherited_preview_fd() -> Option<OwnedFd> {
    if memfd_disabled_by_env() {
        return None;
    }
    let raw_str = std::env::var(ENV_PREVIEW_FD).ok()?;
    let fd: RawFd = raw_str.trim().parse().ok()?;
    if fd < 3 {
        return None;
    }
    let mut sock_type: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `sock_type` and `len` are valid stack pointers for `getsockopt(SO_TYPE)`.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut sock_type as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    if rc != 0 || sock_type != libc::SOCK_SEQPACKET {
        return None;
    }
    // SAFETY: `fd` is a validated open socket descriptor; mark close-on-exec before spawning helpers.
    let _ = unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    // SAFETY: `fd` was exclusively inherited from `omastudio-app` via `OMASTUDIO_PREVIEW_FD`.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Tear-free snapshot read from a `memfd` slot on the consumer (`omastudio-app`) side.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ReadFrame {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: PixelFormat,
    pub color_space: ColorSpaceTag,
    pub pixels: Vec<u8>,
}

/// Consumer-side read-only `memfd` ring owned by `omastudio-app`'s `QQuickImageProvider`.
#[allow(dead_code)]
pub struct PreviewRingReader {
    slots: Vec<MmapSlot>,
}

#[allow(dead_code)]
impl PreviewRingReader {
    /// Receives the initial `SCM_RIGHTS` descriptor packet from `sock_fd`, validates `F_SEAL_SHRINK`
    /// and backing file sizes, maps all 8 slots read-only, and sends `RING_ACK_OK`.
    /// Returns `Ok(None)` cleanly on peer EOF.
    pub fn recv_and_ack(sock_fd: RawFd) -> io::Result<Option<Self>> {
        let Some((payload, fds)) = recv_fds(sock_fd)? else {
            return Ok(None);
        };
        let Some(caps) = decode_init_msg(&payload) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid preview ring init header",
            ));
        };
        if fds.len() != TOTAL_SLOTS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected {TOTAL_SLOTS} memfd slots, got {}", fds.len()),
            ));
        }

        let mut slots = Vec::with_capacity(TOTAL_SLOTS);
        for (fd, cap) in fds.into_iter().zip(caps) {
            // Require F_SEAL_SHRINK so the producer can never shrink the memfd and cause SIGBUS.
            // SAFETY: `fd` is an open descriptor received via `SCM_RIGHTS`.
            let seals = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GET_SEALS) };
            if seals < 0 || (seals & libc::F_SEAL_SHRINK) == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "received memfd slot is missing F_SEAL_SHRINK",
                ));
            }
            // SAFETY: `libc::stat` is a plain C struct safe to zero-initialize before `fstat`.
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: `fd` is open and `st` is a valid `libc::stat` buffer.
            if unsafe { libc::fstat(fd.as_raw_fd(), &mut st) } != 0 {
                return Err(io::Error::last_os_error());
            }
            if st.st_size < 0 || (st.st_size as usize) < cap {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "received memfd slot is smaller than declared capacity",
                ));
            }
            slots.push(MmapSlot::map(fd, cap, false)?);
        }

        // Acknowledge after all slots are verified and mapped so no image:// URL can race setup.
        // SAFETY: `RING_ACK_OK` is a valid 4-byte buffer; `MSG_NOSIGNAL` avoids SIGPIPE.
        let sent = unsafe {
            libc::send(
                sock_fd,
                RING_ACK_OK.as_ptr().cast(),
                RING_ACK_OK.len(),
                libc::MSG_NOSIGNAL,
            )
        };
        if sent != 4 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(Self { slots }))
    }

    /// Reads a consistent, tear-free frame copy from `(kind, local_slot)` via the seqlock header.
    pub fn read_frame(&self, kind: SlotKind, local_slot: usize) -> Option<ReadFrame> {
        if local_slot >= SLOTS_PER_KIND {
            return None;
        }
        let slot = self.slots.get(kind.base_slot() + local_slot)?;
        let hdr = slot.header();

        for attempt in 0..64u32 {
            let seq1 = hdr.seq.load(Ordering::Acquire);
            if seq1 == 0 {
                return None;
            }
            if seq1 & 1 != 0 {
                if attempt < 16 {
                    std::hint::spin_loop();
                } else {
                    std::thread::yield_now();
                }
                continue;
            }

            let generation = hdr.generation.load(Ordering::Relaxed);
            let width = hdr.width.load(Ordering::Relaxed);
            let height = hdr.height.load(Ordering::Relaxed);
            let stride = hdr.stride.load(Ordering::Relaxed);
            let fmt_raw = hdr.format.load(Ordering::Relaxed);
            let cs_raw = hdr.color_space.load(Ordering::Relaxed);
            fence(Ordering::Acquire);

            if hdr.seq.load(Ordering::Acquire) != seq1 {
                continue;
            }

            let format = PixelFormat::from_u32(fmt_raw)?;
            let color_space = ColorSpaceTag::from_u32(cs_raw)?;
            if generation == 0 || width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
                return None;
            }
            let min_stride = width.checked_mul(format.bytes_per_pixel())?;
            if stride < min_stride {
                return None;
            }
            let payload_len = (stride as usize).checked_mul(height as usize)?;
            if payload_len > slot.payload_capacity() || payload_len > MAX_FRAME_BYTES {
                return None;
            }

            let mut pixels = vec![0u8; payload_len];
            // SAFETY: `payload_len <= slot.payload_capacity()` and `pixels` is a distinct heap buffer;
            // we re-check `seq1 == seq2` after an acquire fence to discard any concurrent overwrite.
            unsafe {
                std::ptr::copy_nonoverlapping(slot.payload_ptr(), pixels.as_mut_ptr(), payload_len);
            }
            fence(Ordering::Acquire);
            let seq2 = hdr.seq.load(Ordering::Acquire);
            if seq1 == seq2 {
                return Some(ReadFrame {
                    generation,
                    width,
                    height,
                    stride,
                    format,
                    color_space,
                    pixels,
                });
            }
        }
        None
    }
}

/// Parses an image provider ID (`"preview/0/42"`, `"detail/1/7"`, etc.) into `(kind, local_slot, gen)`.
#[allow(dead_code)]
pub fn parse_provider_id(id: &str) -> Option<(SlotKind, usize, u64)> {
    let clean = id.split('?').next().unwrap_or(id).trim_matches('/');
    let mut parts = clean.split('/');
    let kind = SlotKind::parse(parts.next()?)?;
    let slot: usize = parts.next()?.parse().ok()?;
    let generation: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || slot >= SLOTS_PER_KIND || generation == 0 {
        return None;
    }
    Some((kind, slot, generation))
}

#[allow(dead_code)]
fn send_fds(sock_fd: RawFd, payload: &[u8], fds: &[RawFd]) -> io::Result<()> {
    let mut iov = libc::iovec {
        iov_base: payload.as_ptr() as *mut libc::c_void,
        iov_len: payload.len(),
    };
    let fd_bytes = std::mem::size_of_val(fds);
    // SAFETY: `CMSG_SPACE` is a pure size calculation for `fd_bytes` of ancillary data.
    let cmsg_space = unsafe { libc::CMSG_SPACE(fd_bytes as u32) as usize };
    let mut cmsg_buf = vec![0u8; cmsg_space];

    // SAFETY: zero-initializing `msghdr` before populating all fields is standard POSIX usage.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr().cast();
    msg.msg_controllen = cmsg_buf.len() as _;

    // SAFETY: `msg.msg_control` has `CMSG_SPACE(fd_bytes)` bytes, sufficient for one `SCM_RIGHTS` header.
    let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    if cmsg.is_null() {
        return Err(io::Error::other("CMSG_FIRSTHDR returned null"));
    }
    // SAFETY: `cmsg` is valid and aligned within `cmsg_buf`; `CMSG_DATA` has room for `fd_bytes`.
    unsafe {
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(fd_bytes as u32) as _;
        std::ptr::copy_nonoverlapping(fds.as_ptr().cast::<u8>(), libc::CMSG_DATA(cmsg), fd_bytes);
    }

    loop {
        // SAFETY: `sock_fd` is an open Unix socket and `msg` points to valid stack/heap buffers.
        let rc = unsafe { libc::sendmsg(sock_fd, &msg, libc::MSG_NOSIGNAL) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if rc as usize != payload.len() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short sendmsg on preview socket"));
        }
        return Ok(());
    }
}

#[allow(dead_code)]
fn recv_fds(sock_fd: RawFd) -> io::Result<Option<(Vec<u8>, Vec<OwnedFd>)>> {
    let mut payload = vec![0u8; INIT_MSG_BYTES + 64];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let max_fd_bytes = TOTAL_SLOTS * std::mem::size_of::<RawFd>();
    // SAFETY: `CMSG_SPACE` is a pure size calculation for `max_fd_bytes`.
    let cmsg_space = unsafe { libc::CMSG_SPACE(max_fd_bytes as u32) as usize };
    let mut cmsg_buf = vec![0u8; cmsg_space];

    // SAFETY: zero-initializing `msghdr` before populating I/O and control buffers.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr().cast();
    msg.msg_controllen = cmsg_buf.len() as _;

    let n = loop {
        // SAFETY: `sock_fd` is an open Unix socket and `msg` points to valid buffers;
        // `MSG_CMSG_CLOEXEC` marks every received file descriptor close-on-exec atomically.
        let rc = unsafe { libc::recvmsg(sock_fd, &mut msg, libc::MSG_CMSG_CLOEXEC) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        break rc as usize;
    };

    if n == 0 {
        return Ok(None);
    }
    payload.truncate(n);

    let mut received = Vec::new();
    // SAFETY: `msg` was populated by a successful `recvmsg` call using `cmsg_buf`.
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    while !cmsg.is_null() {
        // SAFETY: `cmsg` is a valid control header returned by `CMSG_FIRSTHDR` / `CMSG_NXTHDR`.
        let (level, ty, cmsg_len) = unsafe { ((*cmsg).cmsg_level, (*cmsg).cmsg_type, (*cmsg).cmsg_len as usize) };
        if level == libc::SOL_SOCKET && ty == libc::SCM_RIGHTS {
            // SAFETY: `CMSG_LEN(0)` is the byte offset to the `cmsg` data array.
            let hdr_len = unsafe { libc::CMSG_LEN(0) as usize };
            if cmsg_len > hdr_len {
                let data_bytes = cmsg_len - hdr_len;
                let count = data_bytes / std::mem::size_of::<RawFd>();
                // SAFETY: `CMSG_DATA(cmsg)` contains `count` valid `RawFd` values from the kernel.
                let data_ptr = unsafe { libc::CMSG_DATA(cmsg) };
                for i in 0..count {
                    let mut raw_fd: RawFd = -1;
                    // SAFETY: copying `size_of::<RawFd>()` bytes from unaligned ancillary data into `raw_fd`.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            data_ptr.add(i * std::mem::size_of::<RawFd>()),
                            (&mut raw_fd as *mut RawFd).cast::<u8>(),
                            std::mem::size_of::<RawFd>(),
                        );
                    }
                    if raw_fd >= 0 {
                        // SAFETY: `raw_fd` was newly created in this process by `recvmsg(SCM_RIGHTS)`.
                        received.push(unsafe { OwnedFd::from_raw_fd(raw_fd) });
                    }
                }
            }
        }
        // SAFETY: advances to the next control message header within `cmsg_buf`.
        cmsg = unsafe { libc::CMSG_NXTHDR(&msg, cmsg) };
    }

    if (msg.msg_flags & libc::MSG_CTRUNC) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SCM_RIGHTS control message truncated",
        ));
    }

    Ok(Some((payload, received)))
}

/// Metadata carried alongside `[dmabuf_fd, (optional) sync_fd]` over `OMASTUDIO_PREVIEW_FD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub struct DmaBufFrameDesc {
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub offset: u32,
    pub vk_format: u32,
    pub drm_format: u32,
    pub drm_modifier: u64,
    pub alloc_size: u64,
    pub color_space: ColorSpaceTag,
    pub has_sync_fd: bool,
    pub device_uuid: [u8; 16],
}

/// Received DMA-BUF frame descriptors + owned file descriptors on the `omastudio-app` side.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ReceivedDmaBufFrame {
    pub desc: DmaBufFrameDesc,
    pub dmabuf_fd: OwnedFd,
    pub sync_fd: Option<OwnedFd>,
}

const DMABUF_MSG_BYTES: usize = 80;

#[allow(dead_code)]
fn encode_dmabuf_msg(desc: &DmaBufFrameDesc) -> [u8; DMABUF_MSG_BYTES] {
    let mut buf = [0u8; DMABUF_MSG_BYTES];
    buf[0..4].copy_from_slice(&RING_MAGIC);
    buf[4..8].copy_from_slice(&RING_VERSION.to_le_bytes());
    buf[8..12].copy_from_slice(&MSG_KIND_DMABUF_FRAME.to_le_bytes());
    buf[12] = desc.color_space as u8;
    buf[13] = u8::from(desc.has_sync_fd);
    buf[16..24].copy_from_slice(&desc.generation.to_le_bytes());
    buf[24..28].copy_from_slice(&desc.width.to_le_bytes());
    buf[28..32].copy_from_slice(&desc.height.to_le_bytes());
    buf[32..36].copy_from_slice(&desc.stride.to_le_bytes());
    buf[36..40].copy_from_slice(&desc.offset.to_le_bytes());
    buf[40..44].copy_from_slice(&desc.vk_format.to_le_bytes());
    buf[44..48].copy_from_slice(&desc.drm_format.to_le_bytes());
    buf[48..56].copy_from_slice(&desc.drm_modifier.to_le_bytes());
    buf[56..64].copy_from_slice(&desc.alloc_size.to_le_bytes());
    buf[64..80].copy_from_slice(&desc.device_uuid);
    buf
}

#[allow(dead_code)]
fn decode_dmabuf_msg(buf: &[u8]) -> Option<DmaBufFrameDesc> {
    if buf.len() != DMABUF_MSG_BYTES || buf[0..4] != RING_MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(buf[4..8].try_into().ok()?);
    let kind = u32::from_le_bytes(buf[8..12].try_into().ok()?);
    if version != RING_VERSION || kind != MSG_KIND_DMABUF_FRAME {
        return None;
    }
    let color_space = ColorSpaceTag::from_u32(buf[12] as u32)?;
    let has_sync_fd = buf[13] != 0;
    let generation = u64::from_le_bytes(buf[16..24].try_into().ok()?);
    let width = u32::from_le_bytes(buf[24..28].try_into().ok()?);
    let height = u32::from_le_bytes(buf[28..32].try_into().ok()?);
    let stride = u32::from_le_bytes(buf[32..36].try_into().ok()?);
    let offset = u32::from_le_bytes(buf[36..40].try_into().ok()?);
    let vk_format = u32::from_le_bytes(buf[40..44].try_into().ok()?);
    let drm_format = u32::from_le_bytes(buf[44..48].try_into().ok()?);
    let drm_modifier = u64::from_le_bytes(buf[48..56].try_into().ok()?);
    let alloc_size = u64::from_le_bytes(buf[56..64].try_into().ok()?);
    let mut device_uuid = [0u8; 16];
    device_uuid.copy_from_slice(&buf[64..80]);

    if generation == 0
        || width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || alloc_size == 0
        || alloc_size > (512 << 20)
    {
        return None;
    }
    Some(DmaBufFrameDesc {
        generation,
        width,
        height,
        stride,
        offset,
        vk_format,
        drm_format,
        drm_modifier,
        alloc_size,
        color_space,
        has_sync_fd,
        device_uuid,
    })
}

/// Transfers a DMA-BUF frame FD (and optional Vulkan `SYNC_FD`) over `sock_fd` via `SCM_RIGHTS`
/// and waits up to `timeout_ms` for `omastudio-app` to park the descriptors (`RING_ACK_OK`).
#[allow(dead_code)]
pub fn send_dmabuf_frame_and_wait_ack(
    sock_fd: RawFd,
    desc: &DmaBufFrameDesc,
    dmabuf_fd: RawFd,
    sync_fd: Option<RawFd>,
    timeout_ms: i32,
) -> io::Result<()> {
    let mut wire_desc = *desc;
    wire_desc.has_sync_fd = sync_fd.is_some();
    let payload = encode_dmabuf_msg(&wire_desc);
    if let Some(sfd) = sync_fd {
        send_fds(sock_fd, &payload, &[dmabuf_fd, sfd])?;
    } else {
        send_fds(sock_fd, &payload, &[dmabuf_fd])?;
    }

    let mut pfd = libc::pollfd {
        fd: sock_fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: `pfd` points to one valid `pollfd` struct.
        let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if rc < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if rc == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for DMA-BUF frame ack",
            ));
        }
        break;
    }

    let mut ack = [0u8; 4];
    // SAFETY: `ack` is a valid 4-byte writable buffer; `sock_fd` is an open Unix socket.
    let n = unsafe { libc::recv(sock_fd, ack.as_mut_ptr().cast(), ack.len(), 0) };
    if n != 4 || ack != RING_ACK_OK {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid DMA-BUF frame ack from host app",
        ));
    }
    Ok(())
}

/// Receives the next DMA-BUF frame descriptor + `SCM_RIGHTS` FDs from `sock_fd`, invokes
/// `on_frame` **before** replying with `RING_ACK_OK` (guaranteeing happens-before visibility
/// before the daemon emits its stdout JSON line), and returns `Ok(false)` on peer EOF.
#[allow(dead_code)]
pub fn recv_dmabuf_frame_with_ack<F>(sock_fd: RawFd, on_frame: F) -> io::Result<bool>
where
    F: FnOnce(ReceivedDmaBufFrame),
{
    let Some((payload, mut fds)) = recv_fds(sock_fd)? else {
        return Ok(false);
    };
    let Some(desc) = decode_dmabuf_msg(&payload) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid DMA-BUF frame descriptor header",
        ));
    };
    let expected_fds = if desc.has_sync_fd { 2 } else { 1 };
    if fds.len() != expected_fds {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {expected_fds} DMA-BUF FDs, got {}", fds.len()),
        ));
    }
    let sync_fd = if desc.has_sync_fd { fds.pop() } else { None };
    let dmabuf_fd = fds
        .pop()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing dmabuf_fd"))?;

    on_frame(ReceivedDmaBufFrame {
        desc,
        dmabuf_fd,
        sync_fd,
    });

    // SAFETY: `RING_ACK_OK` is a valid 4-byte buffer; `MSG_NOSIGNAL` avoids SIGPIPE.
    let sent = unsafe {
        libc::send(
            sock_fd,
            RING_ACK_OK.as_ptr().cast(),
            RING_ACK_OK.len(),
            libc::MSG_NOSIGNAL,
        )
    };
    if sent != 4 {
        return Err(io::Error::last_os_error());
    }
    Ok(true)
}

/// Receives the next DMA-BUF frame descriptor + `SCM_RIGHTS` FDs from `sock_fd` and replies
/// with `RING_ACK_OK`. Returns `Ok(None)` cleanly on peer EOF or socket shutdown.
#[allow(dead_code)]
pub fn recv_dmabuf_frame_and_ack(sock_fd: RawFd) -> io::Result<Option<ReceivedDmaBufFrame>> {
    let mut out = None;
    if recv_dmabuf_frame_with_ack(sock_fd, |frame| out = Some(frame))? {
        Ok(out)
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socketpair_scm_rights_memfd_roundtrip_all_formats() {
        let (parent, child) = create_preview_socketpair().expect("socketpair");

        let handle = std::thread::spawn(move || {
            PreviewRingReader::recv_and_ack(parent.as_raw_fd())
                .expect("recv_and_ack")
                .expect("ring present")
        });

        let mut writer = PreviewRingWriter::create().expect("create memfd ring");
        writer
            .send_and_wait_ack(child.as_raw_fd(), 5_000)
            .expect("send_and_wait_ack");
        let reader = handle.join().expect("reader thread");

        // 1. RGB888 -> RGBA8888 preview frame
        let rgb = vec![10u8, 20, 30, 200, 150, 100];
        let pub1 = writer
            .write_rgb8_as_rgba8(SlotKind::Preview, &rgb, 2, 1, ColorSpaceTag::DisplayP3)
            .expect("publish preview");
        assert_eq!(pub1.url, "image://omastudio/preview/0/1");
        assert_eq!(pub1.stride, 8);
        assert_eq!(pub1.format, PixelFormat::Rgba8888);
        assert_eq!(pub1.color_space, ColorSpaceTag::DisplayP3);

        let (kind, slot, generation) = parse_provider_id("preview/0/1").expect("parse id");
        assert_eq!((kind, slot, generation), (SlotKind::Preview, 0, 1));
        let frame = reader.read_frame(kind, slot).expect("read preview");
        assert_eq!(frame.generation, 1);
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 1);
        assert_eq!(frame.stride, 8);
        assert_eq!(frame.format, PixelFormat::Rgba8888);
        assert_eq!(frame.color_space, ColorSpaceTag::DisplayP3);
        assert_eq!(frame.pixels, vec![10, 20, 30, 255, 200, 150, 100, 255]);

        // 2. Second preview alternates to slot 1
        let pub2 = writer
            .write_rgb8_as_rgba8(SlotKind::Preview, &rgb, 2, 1, ColorSpaceTag::Srgb)
            .expect("publish second preview");
        assert_eq!(pub2.url, "image://omastudio/preview/1/2");

        // 3. Raw RGBA16F HDR frame in Reference slot (ready for WP-16)
        let fp16_bytes = vec![0x00, 0x3c, 0x00, 0x3c, 0x00, 0x3c, 0x00, 0x3c]; // 1x1 white in half-float
        let pub3 = writer
            .write_raw(
                SlotKind::Reference,
                &fp16_bytes,
                1,
                1,
                8,
                PixelFormat::Rgba16F,
                ColorSpaceTag::Bt2100Pq,
            )
            .expect("publish fp16 reference");
        assert_eq!(pub3.url, "image://omastudio/reference/0/3");
        let ref_frame = reader.read_frame(SlotKind::Reference, 0).expect("read fp16");
        assert_eq!(ref_frame.format, PixelFormat::Rgba16F);
        assert_eq!(ref_frame.color_space, ColorSpaceTag::Bt2100Pq);
        assert_eq!(ref_frame.pixels, fp16_bytes);

        // 4. Loupe slot roundtrip
        let pub4 = writer
            .write_rgb8_as_rgba8(SlotKind::Loupe, &rgb, 2, 1, ColorSpaceTag::Srgb)
            .expect("publish loupe");
        assert_eq!(pub4.url, "image://omastudio/loupe/0/4");
        assert_eq!(reader.read_frame(SlotKind::Loupe, 0).unwrap().generation, 4);
    }

    #[test]
    fn dmabuf_frame_scm_rights_roundtrip_with_sync_fd() {
        let (parent, child) = create_preview_socketpair().expect("socketpair");
        // Use two dummy eventfds to exercise exact 2-FD SCM_RIGHTS + ack framing cleanly
        // SAFETY: `eventfd(0, EFD_CLOEXEC)` creates a valid standalone Linux descriptor.
        let fake_dmabuf = unsafe { OwnedFd::from_raw_fd(libc::eventfd(0, libc::EFD_CLOEXEC)) };
        // SAFETY: `eventfd(1, EFD_CLOEXEC)` creates a signalled Linux descriptor.
        let fake_sync = unsafe { OwnedFd::from_raw_fd(libc::eventfd(1, libc::EFD_CLOEXEC)) };

        let desc = DmaBufFrameDesc {
            generation: 7,
            width: 1920,
            height: 1080,
            stride: 1920 * 8,
            offset: 0,
            vk_format: VK_FORMAT_R16G16B16A16_SFLOAT,
            drm_format: DRM_FORMAT_ABGR16161616F,
            drm_modifier: 0,
            alloc_size: 1920 * 1080 * 8,
            color_space: ColorSpaceTag::DisplayP3,
            has_sync_fd: true,
            device_uuid: [0x11; 16],
        };

        let rx = std::thread::spawn(move || {
            recv_dmabuf_frame_and_ack(parent.as_raw_fd())
                .expect("recv dmabuf")
                .expect("some frame")
        });

        send_dmabuf_frame_and_wait_ack(
            child.as_raw_fd(),
            &desc,
            fake_dmabuf.as_raw_fd(),
            Some(fake_sync.as_raw_fd()),
            5_000,
        )
        .expect("send dmabuf");

        let got = rx.join().expect("join");
        assert_eq!(got.desc, desc);
        assert!(got.dmabuf_fd.as_raw_fd() >= 0);
        assert!(got.sync_fd.is_some());
    }

    #[test]
    fn preview_mode_env_parsing() {
        assert_eq!(parse_preview_mode(None), PreviewMode::Auto);
        assert_eq!(parse_preview_mode(Some("auto")), PreviewMode::Auto);
        assert_eq!(parse_preview_mode(Some("dma-buf")), PreviewMode::DmaBuf);
        assert_eq!(parse_preview_mode(Some("MEMFD")), PreviewMode::Memfd);
        assert_eq!(parse_preview_mode(Some("shm")), PreviewMode::File);
    }

    #[test]
    fn provider_id_parser_rejects_malformed_ids() {
        assert_eq!(parse_provider_id("detail/1/99?v=2"), Some((SlotKind::Detail, 1, 99)));
        assert_eq!(parse_provider_id("preview/2/1"), None);
        assert_eq!(parse_provider_id("preview/0/0"), None);
        assert_eq!(parse_provider_id("unknown/0/1"), None);
        assert_eq!(parse_provider_id("preview/0/1/extra"), None);
        assert_eq!(parse_provider_id("../etc/passwd"), None);
    }

    #[test]
    fn oversized_loupe_frame_returns_none_cleanly() {
        let mut writer = PreviewRingWriter::create().expect("create ring");
        // Loupe slot capacity is 8 MiB; 2000x2000 RGBA8 = 16 MiB must be rejected cleanly
        let dummy = vec![0u8; 2000 * 2000 * 3];
        assert!(
            writer
                .write_rgb8_as_rgba8(SlotKind::Loupe, &dummy, 2000, 2000, ColorSpaceTag::Srgb)
                .is_none()
        );
    }
}
