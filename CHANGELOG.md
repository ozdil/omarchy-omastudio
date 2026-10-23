# Changelog

## Unreleased

### Languages
- The interface is translated into 30 languages, each one complete: Arabic, Chinese (Simplified and Traditional), Czech, Danish, Dutch, English, Finnish, French, German, Greek, Hebrew, Hindi, Hungarian, Indonesian, Italian, Japanese, Korean, Norwegian Bokmål, Persian, Polish, Portuguese (Brazil), Romanian, Russian, Spanish, Swedish, Thai, Turkish, Ukrainian and Vietnamese.
- OmaStudio follows the system language; the language menu in the top bar picks another one and applies it without a restart (stored in `~/.config/omastudio/language`).
- Arabic, Hebrew and Persian are laid out right to left.
- `tools/i18n/check.py` (run in CI) rejects missing translations, English left in a translation, wrong scripts and mismatched placeholders, so no language shows a mixed interface.
- The `omastudio` launcher passes the user's language through to the application.
- Building needs `qt6-tools` for `lrelease`.

### Omarchy integration
- The interface follows Omarchy live, without a restart: every colour comes from the active theme (scopes, the area around the photo and text on accent buttons included); the font is Omarchy's (`omarchy-font-set`, read from fontconfig); text sizes scale from the Omarchy shell's base size; corners follow Hyprland's rounding, so they are square on a default Omarchy setup.
- Clipping warnings stay red (highlights) and blue (shadows) in every theme.
- OmaStudio has its own icon instead of the generic camera icon.
- The top bar no longer repeats "STUDIO" next to the name.
- New About window: application icon, version, capabilities, graphics and appearance details, a privacy note, third-party licences, project links, and **Copy system info** for bug reports. Esc closes it.

### Editing screen
- The right panel has tabs: **Adjust** (profile, white balance, tone, presence; in Pro also curves, HSL, colour grading, detail, grain and colour management), **Masks** (Pro), **Looks** (presets and 3D LUTs) and **AI** (the edit assistant). Each section folds open and closed from its header; Ctrl+click keeps only that section open. The tab, the open sections and which panels are shown are remembered between sessions (`~/.config/omastudio/ui-state.json`).
- Tab hides and shows both side panels and the filmstrip together.
- Plain names everywhere: section titles are short and in sentence case, and names of other products are gone from the interface. Messages no longer carry tags such as "[OK]".
- Profiles are a compact list instead of large cards; the description is in the tooltip.
- 3D LUTs: **Load .cube file…** opens your own LUT. The four built-in named looks are no longer offered; edits that use them still render.
- Auto is a single action (the separate "Jev" auto was not reachable from the interface and has been folded in).
- The gesture hint over the photo and the "DaVinci Color Engine" line are gone.
- Each control has one place: zoom (with the current percentage) and rotate are on the bar under the photo; undo, redo, before/after and crop are in the top bar. Copy, paste, reset and XMP are a fixed row under the panel, the same in every tab.
- Fujifilm has its own section: Color Chrome for Fujifilm photos, and recipes, the camera over USB and Pixel Shift. It is also shown with no photo open, so a camera can be connected.
- With no photo open, editing sections and buttons are dimmed and inactive.
- The area around the photo is neutral dark grey by default. The button under the photo switches to middle grey or the theme colour; the choice is remembered.
- Rating and flags use star, flag and reject icons with tooltips. Nothing is drawn smaller than 10 px at the base font size.
- Section reset buttons sit in the section header only. Disabled buttons look disabled.
- "Temperature" and "Tint" instead of "Temp (Kelvin)" and "Tint (Green / Magenta)".
- The scene guess is shown only when it is at least 90 % sure, labelled as a suggestion.

### Curves
- The Tone curve section has a point curve editor over the histogram: an RGB curve and one each for red, green and blue. Click adds a point, dragging moves it, double-click removes it; the other channels' curves show faintly behind the one being edited. The curve is a monotone cubic through the points, so it never overshoots them; values above white keep their headroom.
- Point curves are stored in the recipe (`curve_points`, `curve_points_red`, `curve_points_green`, `curve_points_blue`) and written to XMP sidecars as Camera Raw's `ToneCurvePV2012` curves, which OmaStudio also reads from Lightroom and Camera Raw sidecars.
- The four region sliders stay below the editor.

### Masks
- New mask kinds: **Brush** (painted on the photo, with size, feather, flow and an erase mode) and **Colour** (colours close to one picked from the photo, with range and feather; compared in Oklab).
- A layer's mask can be built from several parts: under **Combine with**, brush, colour, luminance, linear and radial parts are added to, subtracted from or intersected with the layer's own mask. Each part can be inverted.
- Masks are edited on the photo: the brush paints with an outline of its size and shows what is painted; the colour picker takes the colour under the pointer; linear and radial masks have handles to drag.
- The luminance mask has its range and falloff controls, and the radial mask its feather, in the panel.
- Layers also adjust highlights, shadows, whites and blacks.

### Heal and clone
- New **Heal and clone** section in the Masks tab. With **Edit spots** on, a click on the photo removes what is under it: the spot is replaced from a source beside it. Drag the solid circle to move the spot and the dashed circle to pick another source; size, feather and opacity apply to the selected spot and to new ones.
- **Heal** matches the copied area to the light and colour around the spot (the edge difference is spread over the spot with the Poisson kernel, so shading carries through); **Clone** copies it as it is.
- Spots are applied to the linear photo before every other step, in the preview, the 1:1 detail view and export alike, and stored in the recipe as `retouch`.

### Transform
- New **Transform** section (Pro mode) with **Vertical**, **Horizontal** and **Scale**. Vertical straightens converging lines, such as a building photographed from below; Horizontal does the same sideways. The photo is turned as a plane about its centre (up to 30°), so the result has the right proportions rather than a stretched keystone.
- Applied with lens distortion in one resampling step, in the preview, the 1:1 detail view and export alike. Masks and heal spots stay where they were placed on the photo; their overlays follow the correction.
- Stored in the recipe as `perspective_vertical`, `perspective_horizontal` and `perspective_scale`. Not yet written to XMP sidecars.

### Colour management
- **Gamut compression** is now a switch of its own and follows the ACES 1.3 reference gamut compression: colours pushed outside the gamut by grading come back in smoothly, colours inside it are left alone. The previous version clipped negative values outright and desaturated every strongly saturated colour.
- The working colour space list (sRGB, Display P3, Rec. 2020, ACEScg) is gone: the choice did not change the rendering (ACEScg only switched gamut compression on), and its coverage figures were wrong. The output colour space is chosen in the Export window, which converts with ICC profiles. Edits saved with ACEScg open with gamut compression on.
- **ACES tone curve** (formerly "ACES 1.3 RRT/ODT Tonemapper") is applied in linear light with the fit's own scaling. It was applied to display-encoded values, which flattened the image and capped white at 80 %. It no longer switches on gamut compression by itself, and it is named for what it is: a fitted approximation of the ACES tone scale.
- The `AcesColorManagement` processing node converts back from ACEScg after its work instead of leaving the buffer in ACEScg primaries.

### Sensors
- Monochrome sensors and linear (already demosaiced) DNG files, such as pixel-shift composites, are developed by OmaStudio's own pipeline: black level, white balance and colour matrix are applied as for Bayer and X-Trans files, instead of handing the file to LibRaw's rendering.
- Previews use the largest embedded JPEG when a file carries several.
- `omastudio-engine cameras [FILTER]` lists the cameras the RAW decoder supports; `status` reports their number.

### Opening photos
- Every RAW format LibRaw reads opens from the dialog, by drag and drop and from the folder view (Fujifilm, Hasselblad, Phase One, Leica, Pentax, Mamiya/Leaf, Canon, Nikon, Sony, Panasonic, Olympus, Sigma, DNG and others), plus JPEG, PNG, WebP, TIFF (8/16-bit and float), HEIF/HEIC, AVIF, JPEG XL, OpenEXR, Radiance HDR and PNM.
- Images with a wide-gamut ICC profile (Display P3, Adobe RGB, ProPhoto) are converted correctly instead of being read as sRGB; scene-linear EXR and HDR files are shown with the sRGB curve.
- The open dialog filters by Photos, RAW or Images, and OmaStudio is offered as an app for these file types.

### Export
- New Export window. Left: format, quality, bit depth, TIFF compression, size, colour profile, rendering intent and metadata. Right: platform presets (crop and output sharpening for Instagram, Stories, X, Facebook, YouTube) and the watermark, which moved here from the side panel, plus the output folder. Enter exports, Esc closes.
- Formats: JPEG, PNG (8/16-bit), TIFF (8/16-bit; none, LZW or ZIP compression), WebP (lossy with `cwebp` from libwebp-utils, lossless at 100; a missing `cwebp` is reported instead of silently writing a lossless file), AVIF (8/10/12-bit; lossless at 100), HEIF (8/10/12-bit with `heif-enc`), JPEG XL (8/16-bit; lossless at 100; written in the container format so camera EXIF is kept) and OpenEXR (half or full float, scene-linear, with the colour space's chromaticities). Every format except EXR carries the ICC profile.
- Exported files are readable by other users (`rw-r--r--`) whichever encoder wrote them.
- ACEScg is a working export profile (it needed an ICC file on disk before).
- Exported JPEG and TIFF files use the `.jpg` and `.tif` extensions.
- Export window: **Choose…** picks the folder; **File name** adds a tag to the name, with a preview of the result. Format tiles show the bit depth, with the use in the tooltip. Clicking an active platform preset again clears it.
- `omastudio ipc showExport` opens the Export window; `setRightTab <adjust|masks|looks|ai>` switches the panel tab.

### AI
- **Connect local AI** in the top bar uses the AI set up on this computer: Omarchy's default agent (`omarchy-default-agent`; Claude Code or Codex, with their own sign-in), else Ollama, else LM Studio. The edit assistant can use it like any other provider (`omastudio-engine ai local`, provider `omarchy`). With Claude Code or Codex the photo preview is sent to that service.
- The "Basit (AI)" mode is now called "Essentials".

## v3.0.0 — 2026-10-22

OmaStudio 3 is a new RAW engine and a standalone Qt 6 application: its own Bayer and X-Trans development on linear light, GPU demosaicing, colour management with Little CMS 2, full Fujifilm support (recipes, film simulations, lens corrections, Pixel Shift, cameras over USB), Hasselblad and other medium-format cameras, AI agents over MCP, and local AI masks.

### Upgrading from 2.x
- OmaStudio now runs as its own window, `omastudio-app`, instead of inside Quickshell. `omastudio ipc <method>` replaces `quickshell ipc call ozdil.omastudio ...`; `~/.local/share/omastudio/qml` is no longer used and can be removed. Quickshell is only needed for the bar button.
- Exposure and white balance now act on linear light. Saved recipes at 0 EV and 5500 K look the same; recipes that change exposure or temperature render somewhat differently.
- Fujifilm RAFs open at the brightness of the camera's own JPEG (baseline exposure), so they look brighter than in 2.x at the same slider values.
- The thumbnail cache is rebuilt once. Ratings, flags and colour labels in `catalog.json` are kept.
- Requires LibRaw 0.22, Qt 6.12 and Little CMS 2; release archives are built on Arch Linux for x86_64.

### Application
- OmaStudio is now a standalone Qt 6 application, `omastudio-app`, built in Rust with cxx-qt. It no longer runs inside Quickshell: the window is a regular Wayland client (app id `omastudio`, so Hyprland window rules can match it) and uses Qt's fractional scaling, so the interface is drawn at the monitor's real pixel density. The QML interface is compiled into the binary; `~/.local/share/omastudio/qml` is no longer used.
- The window listens on a control socket in `$XDG_RUNTIME_DIR/omastudio/` that only the same user can reach. `omastudio ipc <method> [args]` replaces `quickshell ipc call ozdil.omastudio ...` with the same methods (openPhoto, applyRecipe, applyPreset, setExposure, getStatus, getTheme, ...), plus `show` and `screenshot <file.png>`. Opening a photo while a window is running hands it to that window.
- The theme follows Omarchy theme changes without the extra polling timer, and the About dialog shows the application's own version.
- Pixel-exact viewport. Zoom percentages now refer to the photo's own pixels on the screen's physical pixels: 100% shows one photo pixel per screen pixel at any scale factor (1.25, 1.5, 2). As soon as the photo is shown larger than the preview, the engine renders the visible area from the full-resolution frame (decoded in the background after a photo opens) at the screen's physical resolution, with the current adjustments, and the viewport draws it unscaled. Local masks, grain, vignetting and lens distortion are positioned in whole-photo coordinates, so the detail matches the export. The 100% and 200% buttons were relative to the fitted size before.
- Fujifilm panel (Pro mode, next to the film simulations). The recipe library lists saved X RAW Studio profiles and recipe text; a recipe applies with one click, and the recipe a Fujifilm RAF or JPEG was shot with can be applied or saved to the library. A camera connected over USB (USB RAW CONV./BACKUP RESTORE) shows its custom settings C1-C7, which can be applied to the open photo, and a library recipe can be written to any slot. Supported cameras convert the open RAF in camera, optionally with the selected recipe. Selecting the RAF frames of a GFX Pixel Shift sequence in the filmstrip merges them into a DNG and opens it. New control socket methods: `fujiRecipes`, `applyFujiRecipe <label|path>` and `applyCameraRecipe [photo]`.
- The export dialog has a rendering intent selector (perceptual, relative colorimetric, saturation, absolute colorimetric) for the conversion to the output ICC profile. Relative colorimetric stays the default.
- Quickshell is now only needed for the OmaStudio button in the Omarchy bar (`Panel.qml`), which no longer depends on the application's files.

### Interchange
- XMP sidecars. "Write XMP" (and `omastudio xmp export <photo>`) writes `DSCF0001.xmp` with the Camera Raw settings that have an OmaStudio equivalent (exposure, tone, presence, parametric curve, HSL, sharpening, noise reduction, vignette, grain, crop and angle), the star rating, reject flag and colour label, plus the complete OmaStudio recipe in its own namespace, so Lightroom, Capture One, darktable and digiKam see the edit and a round trip loses nothing. "Import XMP" (and `xmp import`) reads `DSCF0001.xmp` or darktable's `DSCF0001.RAF.xmp`. An XMP written by another application is never replaced without `--force`. White balance is only exchanged as "As Shot", because OmaStudio's temperature is relative to the camera's. The MCP server has `read_xmp` and `write_xmp`.

### AI agents
- `omastudio mcp`: a Model Context Protocol server on stdin/stdout with 28 tools. Engine tools inspect, render (the preview comes back as an image), export, scan and cull photos, read and save Fujifilm recipes, merge Pixel Shift sequences and use a Fujifilm camera on USB. Window tools read the window's state, take a screenshot, open photos and apply recipes, presets, exposure, white balance and zoom through the control socket.
- AI edit assistant (panel under Film Looks). Describe the edit in plain words ("warmer, recover the sky") or leave it empty; a vision model sees the photo as currently developed with its camera data and sets exposure, white balance, tone, presence, vignette, grain and film simulation. The answer is constrained to a JSON schema and clamped to each control's range, and it can be undone like any edit. Providers: Anthropic Claude (Claude Opus 5.5 by default), OpenAI, OpenRouter, Google Gemini, and local Ollama or LM Studio servers, which need no key and keep the photo on the machine. API keys are stored in the desktop keyring through `secret-tool` (or read from `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`, `GEMINI_API_KEY`) and reach curl on stdin, never on a command line. Command line: `omastudio ai providers`, `ai key <provider>`, `ai use <provider> [model]`, `ai edit <photo> --prompt "..."`; control socket: `aiEdit <request>`.
- Local AI masks. "+ AI SUBJECT" and "+ AI SKY" in the local adjustment layers panel run a segmentation model on the machine and add a layer masked by its result (a new `bitmap` mask type: a grayscale image in `~/.local/share/omastudio/masks`, sampled over the whole frame, so it also lines up at 100%). Models run through ONNX Runtime, which is loaded at run time and is optional: on an NPU first (Intel Core Ultra through OpenVINO, Snapdragon X through QNN, AMD Ryzen AI through Vitis AI), then a GPU (CUDA, MIGraphX, OpenVINO, WebGPU), then the CPU, using whichever execution providers the installed ONNX Runtime includes. Nothing is downloaded: models are ONNX files the user places in `~/.local/share/omastudio/models` as `subject.onnx` and `sky.onnx` (input `[1, 3, H, W]`, output a mask; optional `<model>.json` with `mean`, `std` and `size`). `omastudio npu` shows the runtime, accelerators and models; `omastudio ai-mask <photo> --kind subject|sky` makes a mask from the command line. `OMASTUDIO_NPU=off` disables local models and `OMASTUDIO_NPU=<provider>` forces one. New control socket method `aiMask subject|sky`; new MCP tools `npu_status` and `ai_mask`.
- An agent skill (`skill/omastudio/SKILL.md`) documents the tools, command line and recipe format; `omastudio skill` installs it into `~/.claude/skills`.

### Toolchain
- Rust 1.99.1 (pinned in `rust-toolchain.toml`) and the Rust 2024 edition. Nested `if let` blocks became let-chains and the new clippy lints are fixed; behaviour is unchanged.
- The Arch package now requires LibRaw 0.22 and Qt 6.12 (`qt6-declarative`), the versions current Omarchy ships, and Little CMS 2 (`lcms2`).
- CI builds, tests and lints in an Arch Linux container, so it uses the same LibRaw, Qt and Rust packages as Omarchy instead of Ubuntu's older ones.

### Engine
- Exposure and white balance now work on linear light. LibRaw and 16-bit TIFF/PNG decodes stay linear until white balance and exposure are applied, and are display-encoded once (with the curve of the output colour space) before the tone and colour modules. Exposure is a true photographic EV step (+1 EV doubles the light) and the white balance multipliers act on light the way the camera's own do. A recipe at 0 EV and 5500 K renders as before; recipes that change exposure or temperature will look somewhat different.
- Faster previews while editing. The colour and tone stage runs from per-channel input tables and, in the editor, from a 65x65x65 table of the creative adjustments rebuilt for each change (exact computation above 90% and for exports); a 6.5 MP preview updates in about 0.5 s instead of 0.65-0.8 s on 4 cores. The editing preview of large files is limited to 3840 pixels on the long side (a 100 MP GFX file was 25 MP before); zoomed views still render from the full-resolution frame. The window sends one render at a time and skips the slider positions passed while the engine was busy, so the image follows the slider instead of replaying a queue.
- Thumbnails are always re-encoded by the engine. A JPEG, PNG or WebP that fails to decode is no longer copied into the thumbnail cache unchanged or shown from its original file; the filmstrip shows a placeholder instead, so a crafted image cannot reach Qt's image decoders without the engine's limits. *(The thumbnail cache is rebuilt once.)*

### RAW development
- OmaStudio develops Bayer and X-Trans RAW files itself. LibRaw now only unpacks the sensor data; black level (including Fujifilm's 6x6 black pattern), white balance, demosaicing and the camera colour matrix run in the engine, straight into the export's colour space.
- New demosaic for X-Trans and Bayer: four directional candidates per pixel, keeping the most homogeneous ones (Markesteijn-style), plus a median on the colour differences against false colour. On a 26 MP X-T30 RAF it matches LibRaw's 3-pass Markesteijn within 45-50 dB PSNR at the same brightness, and a full-resolution export is about 25% faster (8.3 s vs 11.0 s on 4 cores).
- Previews of X-Trans files still use LibRaw's half-size decode (larger than a 3x3 binned preview, same colours). Fixed: these previews failed once the own engine had looked at the file, because LibRaw unpacked the sensor data a second time. Sensors the new path doesn't handle (Foveon, linear DNG, Fuji SuperCCD, 4-colour CFAs) fall back to LibRaw automatically. `OMASTUDIO_DEMOSAIC=libraw` switches back to LibRaw's processing entirely.
- Demosaicing runs on the GPU. A new wgpu compute path (Vulkan on NVIDIA, AMD/RADV, Intel/ANV and Qualcomm Adreno/Turnip; OpenGL ES 3.1 where Vulkan is missing) mirrors the CPU demosaic stage by stage and gives the same result (at most 1 level in 65535 apart). Large frames are processed in bands sized to the GPU's buffer limits, so a 100 MP GFX file also works on integrated graphics. Without a usable GPU, or if a GPU job fails, the CPU path is used. Software renderers (llvmpipe) are skipped since the CPU is faster than them. `omastudio-engine gpu` shows the GPU in use and `OMASTUDIO_GPU=off` turns it off; `status` now reports the real device instead of a guess from installed files. The Arch package lists `vulkan-icd-loader` as an optional dependency.
- Large files need much less memory. The demosaic converts each tile straight to 16-bit output (no full-frame float copy), and the master export processes the photo in bands of rows with enough context for every neighbourhood filter, so the result is unchanged. A full-resolution export of a 26 MP X-T30 RAF peaks at 440 MB instead of 1.2 GB; a 100 MP GFX file needs about 1.7 GB instead of about 4.7 GB.

### Fujifilm
- Photos from Fujifilm cameras open with the recipe they were shot with. The engine reads the MakerNote of RAF and Fujifilm JPEG files itself: film simulation (including Acros/monochrome filters and Reala Ace, Bleach Bypass and Nostalgic Neg ids), highlight and shadow tone, colour, sharpness, noise reduction, clarity, grain, Color Chrome, white balance shift and dynamic range (DR200/DR400). A saved sidecar still wins.
- X RAW Studio profiles (.FP1, .FP2, .FP3) and recipes shared as text ("Film Simulation: Classic Chrome / Grain Effect: Strong, Small / White Balance: Daylight, +3 Red & -5 Blue ...") can be imported, and any recipe can be written back as an .FP1 for X RAW Studio.
- Recipe library in `~/.local/share/omastudio/fuji-recipes`. New commands: `fuji-recipe <raf|jpg|fp1|txt>`, `fuji-recipe list`, `fuji-recipe save <file> [--label]` and `fuji-recipe export <file> <out.fp1> [--device]`; the daemon gains `fuji_recipes` and `fuji_import`. Saving never overwrites a recipe with the same name.
- New Color Chrome Effect and Color Chrome FX Blue sliders (Weak = 50, Strong = 100): saturated colours, and blues for FX Blue, get deeper while keeping their hue.
- New film simulations: Reala Ace, PRO Neg. Std, Eterna Bleach Bypass, Monochrome (plain and with yellow, red and green filters) and Sepia. Nostalgic Neg. and PRO Neg. Hi are now in the film simulation panel as well.
- Film simulation calibration: `fuji-calibrate <raf>... [--any] [--dry-run]` compares the engine's neutral development of each RAF with the JPEG the camera embedded in it and fits a 33x33x33 LUT per film simulation (`~/.local/share/omastudio/film-luts/<simulation>.cube`). Calibrated LUTs replace the built-in approximation automatically; `OMASTUDIO_FILM_LUTS=off` turns them off. On an X-T30 Classic Chrome sample the held-out error falls from 28.2 to 4.0 (8-bit RGB distance). The LUTs are made from your own photos; no Fujifilm data is shipped.
- Lens corrections from the camera. Fujifilm cameras record distortion, lateral chromatic aberration and vignetting data for the mounted lens in every RAF; OmaStudio now reads it and applies it to every decode, as the camera does for its JPEGs (XF, XC and GF lenses alike). On an X-T30 sample the developed image now lines up with the camera's JPEG to within half a pixel (2.8 px before). `OMASTUDIO_LENS_CORRECTION=off` turns this off; the manual distortion slider still works on top.
- GFX Pixel Shift Multi-Shot: `fuji-pixel-shift <raf>... [--out <file.dng>]` merges a sequence into a linear DNG, like Fujifilm's Pixel Shift Combiner. Each RAF's recorded sensor offset places its photosites on a common grid: four frames give measured red, green and blue at every pixel (no demosaicing), 16 frames with half-photosite steps double the resolution (400 MP from a GFX100-series sensor). The direction of the offsets is checked against the images themselves. The DNG holds linear Rec. 2020 with the as-shot white balance, and the camera's lens corrections as standard DNG opcodes (WarpRectilinear, FixVignetteRadial). Moving subjects are not compensated.
- DNG files with lens-correction opcodes (OpcodeList3 WarpRectilinear and FixVignetteRadial, centred) are now corrected when opened.
- Fujifilm RAFs open at the brightness the camera intended. Fujifilm cameras expose the sensor to protect highlights and brighten the image while rendering it, by an amount that depends on the model, the dynamic range setting and the scene, so developments came out about a stop darker than the camera's JPEG. When a photo opens, the engine now develops a small copy with the camera's recipe and finds the exposure at which its median brightness matches the JPEG stored in the same RAF (about 0.1 s), and applies it as a baseline before the recipe's own exposure. The exposure slider still starts at 0, and the preview, zoomed views and export use the same value. `OMASTUDIO_BASELINE_EXPOSURE=off` turns it off.
- Fixed: the film simulation curves lifted black to about 20% and held white at about 75% before rescaling, which made deep shadows turn strongly coloured (a near-black blue became saturated blue in Classic Chrome). Every curve now keeps black at 0 and white at 1.
- Compatibility check: `fuji-compat <raf>... [--full]` reports for each file how the sensor is read (X-Trans or Bayer, own development or LibRaw, colour matrix, lens data), which recorded settings are only approximated, and how close the development with the camera's recipe comes to the JPEG embedded in the RAF (mean difference, brightness ratio, per-channel bias; vertical shots are compared in both rotations). A file that fails is reported and the others continue.

### Cameras over USB
- New PTP/USB layer (`ptp` module, pure Rust via nusb, no libusb or gphoto2): sessions, device info, storage and object transfer, capture, device properties and events. It is the base for tethering and in-camera RAW conversion.
- Fujifilm in-camera RAW conversion, the protocol X RAW Studio uses: `fuji-camera convert <raf> [--recipe <fp1|txt|raf>]` uploads the RAF, applies the recipe to the camera's conversion profile and saves the camera's own JPEG. `fuji-camera presets` reads custom settings C1-C7 and `fuji-camera write-preset <slot> <recipe>` stores a recipe in one. `fuji-camera list` and `info` show connected cameras.
- `tools/fuji-usb`: a capture script and decoder for documenting the USB protocol of more Fujifilm cameras by black-box observation (usbmon while X RAW Studio runs in a VM), with timestamped notes and per-setting data dumps. Not installed by the package.
- The package installs a udev rule (`70-omastudio-cameras.rules`) that gives the logged-in user access to Fujifilm cameras in PTP mode.

### Medium format
- Raws from Hasselblad (3FR, FFF), Phase One and Leaf (IIQ, MOS), Mamiya (MEF) and Pentax 645 (PEF) open in the browser, the file dialog and the engine. Hasselblad files are cropped to the camera's DefaultCrop, so the dark border photosites no longer show; Phase One files get the back's own sensor calibration (column black levels, defect map, split-sensor gains), which fixes the magenta cast on IQ4 files.
- `omastudio-engine medium-format info <raw>` reads what Phocus and Capture One read next to the sensor data: Hasselblad MakerNote (model, crop) and the Phase One directory (back, firmware, body, lens, sensor temperature).
- Lens corrections for medium format: a flat field (LCC) from a shot of an evenly lit white target corrects light falloff and colour cast per channel (`medium-format lcc-create <raw>`, written as `lcc.json` for the folder or `<raw>.lcc.json` for one file); clipped or underexposed calibration shots are refused. `--lensfun` also writes a Lensfun profile for the lens. Lensfun profiles from the system, `~/.local/share/lensfun` and `~/.local/share/omastudio/lenses` are applied to medium format files. `OMASTUDIO_LENS_CORRECTION=off` turns this off.
- 4-shot multi-shot merge for pixel-shift backs: `medium-format multishot f1 f2 f3 f4 --out merged.tif` gives full RGB at every pixel without demosaicing.
- Experimental Hasselblad tethering over Phocus's vendor USB interface (`medium-format tether list|info|iso|capture --experimental`) and a udev rule for Hasselblad and DJI USB ids. It is untested on hardware.
- `docs/medium-format.md` documents the file formats and protocols, worked out clean-room from sample files and from how Phocus, Capture One and Pentax Digital Camera Utility behave. No vendor code, colour tables or lens data are included.

### Colour & Export
- Colour management with Little CMS 2. The engine builds sRGB, Display P3, Adobe RGB (1998), ProPhoto RGB and Rec. 2020 profiles itself and embeds the profile directly in PNG, JPEG, TIFF, WebP, JXL and AVIF files. Wide-gamut export no longer needs exiftool, and Display P3 and Rec. 2020 are new export options.
- Custom RGB ICC profiles (printer, lab or display profiles in `~/.local/share/omastudio/icc`) are real conversions now: the photo is rendered in ProPhoto RGB and converted with lcms2, with black point compensation and a choice of rendering intent (`rendering_intent`, relative colorimetric by default). Non-RGB profiles are refused with a clear error.
- JPEG, PNG and other raster sources exported to a wide-gamut profile are converted from sRGB instead of falling back to sRGB.
- Profiles such as scRGB Linear and e-sRGB are no longer mistaken for sRGB.

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
