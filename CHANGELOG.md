# Changelog

## v2.4.0 (2026-10-16)

A reliability release: crash fixes in the engine, correct colour in exports, and a UI that keeps its state.

### Fixed
- **Crashes:** monochrome RAW files, non-ASCII hex colours and invalid UTF-8 on the daemon's input no longer stop the engine.
- **Colour:** Adobe RGB, ProPhoto, Display P3 and Rec.2020 exports are rendered in that colour space instead of being sRGB tagged as wide gamut. Without exiftool or the ICC file, exports fall back to sRGB.
- **Colour:** the "before" half of the split view no longer looks washed out, HNCS film simulation no longer turns highlights black, and the auto white balance tint has the right sign.
- **Permissions:** exporting or saving no longer changes `$HOME`, `~/Pictures` or the export folder to mode 0700. Exports, sidecars and `render --out` work in symlinked folders and on NAS or USB drives.
- **Catalog:** `inspect` keeps existing stars, flags and colour labels.
- **Recipes:** partial recipes and layers load with the right defaults, and `highlight_reconstruct` and `filmic_agx` survive saves from the UI.
- **OmaID:** the fallback QR code is a real, scannable code, and the PNG is created even without `qrencode`.
- **Export:** batch exports of same-named files no longer overwrite each other, AVIF falls back to ffmpeg, and the EXIF orientation is reset.
- **UI:** the engine is found in packaged installs, previews update when switching photos, sliders set to 0 keep their value, panels stay in sync after undo, paste, AI and reset, and every edit is an undo step.
- **UI:** the export dialog's watermark switch works, batch errors no longer leave the spinner running, and `colors.toml` theme colours apply.
- **Thumbnails:** RAW+JPEG pairs and same-named files in different folders no longer share a thumbnail.
- **Launcher:** the app opens from the app menu (`Exec=` was missing), and HiDPI, cursor and D-Bus settings reach the UI.
- **OmaSend web:** the copy button no longer breaks on quotes.

### Added
- Shortcuts: Ctrl+O (open), Ctrl+0 (fit), Ctrl+1 (100%), Ctrl+←/→ (previous/next photo).
- Shift-click range selection in the filmstrip, and browsing into subfolders.
- An empty-viewport hint, elided long filenames, undo/redo tooltips and red export errors.
- OmaSend modals close with Esc or a click outside; the page is marked as a demo.
- CI runs tests and clippy on every pull request.

### Packaging
- The PKGBUILD and release archives install the ICC profiles, `omastudio-dashboard` and `omastudio-status`.
- New optional dependencies: `perl-image-exiftool`, `ffmpeg`, `qrencode`.
- Note: thumbnails are stored under new names, so the cache is rebuilt on first launch.
