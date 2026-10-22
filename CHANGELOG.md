# Changelog

## Unreleased

### Toolchain
- Rust 1.99.1 (pinned in `rust-toolchain.toml`) and the Rust 2024 edition. Nested `if let` blocks became let-chains and the new clippy lints are fixed; behaviour is unchanged.
- The Arch package now requires LibRaw 0.22 and Qt 6.12 (`qt6-declarative`), the versions current Omarchy ships, and Little CMS 2 (`lcms2`).
- CI builds, tests and lints in an Arch Linux container, so it uses the same LibRaw, Qt and Rust packages as Omarchy instead of Ubuntu's older ones.

### Engine
- Exposure and white balance now work on linear light. LibRaw and 16-bit TIFF/PNG decodes stay linear until white balance and exposure are applied, and are display-encoded once (with the curve of the output colour space) before the tone and colour modules. Exposure is a true photographic EV step (+1 EV doubles the light) and the white balance multipliers act on light the way the camera's own do. A recipe at 0 EV and 5500 K renders as before; recipes that change exposure or temperature will look somewhat different.
- Thumbnails are always re-encoded by the engine. A JPEG, PNG or WebP that fails to decode is no longer copied into the thumbnail cache unchanged or shown from its original file; the filmstrip shows a placeholder instead, so a crafted image cannot reach Qt's image decoders without the engine's limits. *(The thumbnail cache is rebuilt once.)*

### RAW development
- OmaStudio develops Bayer and X-Trans RAW files itself. LibRaw now only unpacks the sensor data; black level (including Fujifilm's 6x6 black pattern), white balance, demosaicing and the camera colour matrix run in the engine, straight into the export's colour space.
- New demosaic for X-Trans and Bayer: four directional candidates per pixel, keeping the most homogeneous ones (Markesteijn-style), plus a median on the colour differences against false colour. On a 26 MP X-T30 RAF it matches LibRaw's 3-pass Markesteijn within 45-50 dB PSNR at the same brightness, and a full-resolution export is about 25% faster (8.3 s vs 11.0 s on 4 cores).
- Previews of X-Trans files still use LibRaw's half-size decode (larger than a 3x3 binned preview, same colours). Fixed: these previews failed once the own engine had looked at the file, because LibRaw unpacked the sensor data a second time. Sensors the new path doesn't handle (Foveon, linear DNG, Fuji SuperCCD, 4-colour CFAs) fall back to LibRaw automatically. `OMASTUDIO_DEMOSAIC=libraw` switches back to LibRaw's processing entirely.
- Large files need much less memory. The demosaic converts each tile straight to 16-bit output (no full-frame float copy), and the master export processes the photo in bands of rows with enough context for every neighbourhood filter, so the result is unchanged. A full-resolution export of a 26 MP X-T30 RAF peaks at 440 MB instead of 1.2 GB; a 100 MP GFX file needs about 1.7 GB instead of about 4.7 GB.

### Fujifilm
- Photos from Fujifilm cameras open with the recipe they were shot with. The engine reads the MakerNote of RAF and Fujifilm JPEG files itself: film simulation (including Acros/monochrome filters and Reala Ace, Bleach Bypass and Nostalgic Neg ids), highlight and shadow tone, colour, sharpness, noise reduction, clarity, grain, Color Chrome, white balance shift and dynamic range (DR200/DR400). A saved sidecar still wins.
- X RAW Studio profiles (.FP1, .FP2, .FP3) and recipes shared as text ("Film Simulation: Classic Chrome / Grain Effect: Strong, Small / White Balance: Daylight, +3 Red & -5 Blue ...") can be imported, and any recipe can be written back as an .FP1 for X RAW Studio.
- Recipe library in `~/.local/share/omastudio/fuji-recipes`. New commands: `fuji-recipe <raf|jpg|fp1|txt>`, `fuji-recipe list`, `fuji-recipe save <file> [--label]` and `fuji-recipe export <file> <out.fp1> [--device]`; the daemon gains `fuji_recipes` and `fuji_import`. Saving never overwrites a recipe with the same name.
- New Color Chrome Effect and Color Chrome FX Blue sliders (Weak = 50, Strong = 100): saturated colours, and blues for FX Blue, get deeper while keeping their hue.
- New film simulations: Reala Ace, PRO Neg. Std, Eterna Bleach Bypass, Monochrome (plain and with yellow, red and green filters) and Sepia. Nostalgic Neg. and PRO Neg. Hi are now in the film simulation panel as well.
- Film simulation calibration: `fuji-calibrate <raf>... [--any] [--dry-run]` compares the engine's neutral development of each RAF with the JPEG the camera embedded in it and fits a 33x33x33 LUT per film simulation (`~/.local/share/omastudio/film-luts/<simulation>.cube`). Calibrated LUTs replace the built-in approximation automatically; `OMASTUDIO_FILM_LUTS=off` turns them off. On an X-T30 Classic Chrome sample the held-out error falls from 28.8 to 4.7 (8-bit RGB distance). The LUTs are made from your own photos; no Fujifilm data is shipped.

### Cameras over USB
- New PTP/USB layer (`ptp` module, pure Rust via nusb, no libusb or gphoto2): sessions, device info, storage and object transfer, capture, device properties and events. It is the base for tethering and in-camera RAW conversion.
- Fujifilm in-camera RAW conversion, the protocol X RAW Studio uses: `fuji-camera convert <raf> [--recipe <fp1|txt|raf>]` uploads the RAF, applies the recipe to the camera's conversion profile and saves the camera's own JPEG. `fuji-camera presets` reads custom settings C1-C7 and `fuji-camera write-preset <slot> <recipe>` stores a recipe in one. `fuji-camera list` and `info` show connected cameras.
- The package installs a udev rule (`70-omastudio-cameras.rules`) that gives the logged-in user access to Fujifilm cameras in PTP mode.

### Colour & Export
- Colour management with Little CMS 2. The engine builds sRGB, Display P3, Adobe RGB (1998), ProPhoto RGB and Rec. 2020 profiles itself and embeds the profile directly in PNG, JPEG, TIFF, WebP, JXL and AVIF files. Wide-gamut export no longer needs exiftool, and Display P3 and Rec. 2020 are new export options.
- Custom RGB ICC profiles (printer, lab or display profiles in `~/.local/share/omastudio/icc`) are real conversions now: the photo is rendered in ProPhoto RGB and converted with lcms2, with black point compensation and a choice of rendering intent (`rendering_intent`, relative colorimetric by default). Non-RGB profiles are refused with a clear error.
- JPEG, PNG and other raster sources exported to a wide-gamut profile are converted from sRGB instead of falling back to sRGB.
- Profiles such as scRGB Linear and e-sRGB are no longer mistaken for sRGB.

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
