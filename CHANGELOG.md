# Changelog

## Unreleased

### Toolchain
- Rust 1.99.1 (pinned in `rust-toolchain.toml`) and the Rust 2024 edition. Nested `if let` blocks became let-chains and the new clippy lints are fixed; behaviour is unchanged.
- The Arch package now requires LibRaw 0.22 and Qt 6.12 (`qt6-declarative`), the versions current Omarchy ships.
- CI builds, tests and lints in an Arch Linux container, so it uses the same LibRaw, Qt and Rust packages as Omarchy instead of Ubuntu's older ones.

### Engine
- Exposure and white balance now work on linear light. LibRaw and 16-bit TIFF/PNG decodes stay linear until white balance and exposure are applied, and are display-encoded once (with the curve of the output colour space) before the tone and colour modules. Exposure is a true photographic EV step (+1 EV doubles the light) and the white balance multipliers act on light the way the camera's own do. A recipe at 0 EV and 5500 K renders as before; recipes that change exposure or temperature will look somewhat different.
- Thumbnails are always re-encoded by the engine. A JPEG, PNG or WebP that fails to decode is no longer copied into the thumbnail cache unchanged or shown from its original file; the filmstrip shows a placeholder instead, so a crafted image cannot reach Qt's image decoders without the engine's limits. *(The thumbnail cache is rebuilt once.)*

### Removed
- OmaSend code that had been copied into this repository: the `rendezvous`, `oma-id-qr` and `force-scan` engine commands, the matching daemon commands (`rendezvous_derive`, `rendezvous_verify`, `pin_verify`), the OmaSend web pages (`assets/web/omasend`, `docs/index.html`) and the `qrcode` dependency. OmaSend lives in [omarchy-omasend](https://github.com/ozdil/omarchy-omasend) and [omasend-web](https://github.com/ozdil/omasend-web). The OmaStudio UI never used these commands.

## v2.4.0 — 2026-10-16

A large stability and correctness release covering the engine, the UI, colour handling, export and CI.

### Engine
- OmaID QR code is now a real, scannable QR code, rendered in-process when `qrencode` is not installed.
- The engine only applies `0700` permissions to directories it creates or owns, never `$HOME` or your export folders.
- Daemon hardening: bounded reads; invalid UTF-8 input no longer stops the daemon.
- Fixed crashes on monochrome (1-channel) RAW files and black/NaN highlights in HNCS film simulations.
- Thumbnails are keyed by full path, so RAW+JPEG pairs and same-named files from different cards no longer share a thumbnail. *(The old thumbnail cache is rebuilt automatically.)*
- Folder scan lists subfolders first and accepts `~`.
- Auto white balance tint sign corrected; AVIF export falls back to `ffmpeg` and reports failures.
- `inspect` keeps existing star rating, flag and colour label in the catalog.

### Colour & Export
- Wide-gamut export really works now: Adobe RGB, ProPhoto, Display P3 and Rec.2020 are rendered in the requested space and get the matching ICC profile. If the profile or `exiftool` is missing, the export falls back to sRGB and is tagged sRGB.
- The Before/After split view now uses the same tone mapping on both halves.
- Export to symlinked folders and external drives owned by another user works.
- Batch export no longer overwrites files that share a name (e.g. `DSC_0001.NEF` and `DSC_0001.JPG`).
- Bundled ICC profiles are installed with the package.

### UI
- Panels stay in sync after loading, undo, paste, AI Jev and reset.
- Slider edits are grouped into undo steps; undo no longer reverts star ratings and flags.
- Layer edits no longer corrupt undo history; deleting a layer keeps the right selection.
- Filmstrip: proper Shift range selection, Ctrl selection includes the open photo, the highlight follows the open photo.
- Export dialog: the watermark switch works, the spinner clears on error, clicks inside dialogs no longer close them.
- Recipe values of `0` are no longer replaced by defaults; theme colours load correctly.
- New shortcuts: `Ctrl+O`, `Ctrl+0` / `Ctrl+1`, `Ctrl+←` / `Ctrl+→`. Also new: an empty-viewport hint, filename eliding, and undo/redo tooltips.

### Launcher & Packaging
- The desktop entry now has `Exec=`, so the app opens from the app menu and opens a photo passed on first launch.
- The launcher passes desktop session variables (D-Bus, Hyprland, cursor theme, Qt scaling) through, fixing wrong cursor and HiDPI scale.
- Packaged installs (`/usr/bin`) find the engine correctly.
- PKGBUILD installs `omastudio-dashboard` / `omastudio-status` and lists optional dependencies (`exiftool`, `ffmpeg`, `qrencode`).

### OmaSend web
- The copy button no longer breaks on quotes; `Esc` closes modals.

### CI
- Build, test and `clippy -D warnings` run on every pull request on a pinned toolchain; the workflow token is read-only.
