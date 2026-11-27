// Single source of truth for supported camera RAW and raster extensions,
// shared between `omastudio-engine` (`src/raw`) and `omastudio-app` (`app/src/host.rs`).

/// Camera RAW extensions LibRaw reads (by file contents; the list decides what
/// folder scans, the file dialog and the desktop entry offer).
pub const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "cs1", "dc2", "dcr", "dcs", "dng", "drf", "eip", "erf",
    "fff", "gpr", "iiq", "k25", "kc2", "kdc", "mdc", "mef", "mfw", "mos", "mrw", "nef", "nrw", "orf", "ori", "pef",
    "ptx", "pxn", "qtk", "raf", "raw", "rdc", "rw2", "rwl", "rwz", "sr2", "srf", "srw", "sti", "x3f",
];

/// Extensions opened as raster images.
pub const RASTER_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "webp", "tif", "tiff", "heic", "heif", "hif", "avif", "jxl", "exr", "hdr", "pnm",
    "ppm", "pgm", "pam",
];
