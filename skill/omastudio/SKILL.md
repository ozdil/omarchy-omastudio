---
name: omastudio
description: Edit, inspect, cull and export RAW photos with OmaStudio on Omarchy, including Fujifilm recipes, cameras on USB and GFX Pixel Shift. Use when the user asks about their photos, RAW development, film simulations or OmaStudio.
---

# OmaStudio

OmaStudio is the RAW photo editor of the user's Omarchy desktop. It has an engine
(`omastudio-engine`) that works on files and a window (`omastudio`) the user edits in.

## How to reach it

1. **MCP server (preferred).** If tools such as `inspect_photo`, `window_status` or
   `render_preview` are available, use them. The server is `omastudio mcp`
   (stdio). To add it to Claude Code: `claude mcp add omastudio -- omastudio mcp`.
2. **Command line.** Every engine command prints one JSON object
   `{"success": bool, "data": ..., "error": ...}`:
   - `omastudio inspect <photo>`: camera, lens, exposure, histogram, scene
   - `omastudio ai-auto <photo>`: suggested recipe
   - `omastudio render <photo> --recipe '<json>' --out /tmp/preview.ppm`
   - `omastudio export <photo> --recipe '<json>' --options '<json>'`
   - `omastudio scan <dir>`, `omastudio catalog query --rating 4`,
     `omastudio catalog cull <photo> --rating 5 --flag pick`
   - `omastudio fuji-recipe <raf|jpg|fp1|txt>`, `fuji-recipe list`, `fuji-recipe save <file> --label "Name"`
   - `omastudio fuji-camera info|presets`, `fuji-camera convert <raf> [--recipe F]`,
     `fuji-camera write-preset <1-7> <recipe-file>`
   - `omastudio fuji-pixel-shift <raf>... --out merged.dng`
   - `omastudio xmp export <photo>` / `xmp import <photo>`: XMP sidecars for Lightroom, Capture One, darktable
   - `omastudio ai edit <photo> --prompt "..." --recipe '<json>'`: the user's configured AI provider proposes a recipe
3. **The open window.** `omastudio ipc <method> [args]` answers with one line:
   `getStatus`, `openPhoto <path>`, `applyRecipe '<json>'`, `setExposure <ev>`,
   `setWarmth <kelvin>`, `applyPreset <name>`, `fujiRecipes`, `applyFujiRecipe <label>`,
   `applyCameraRecipe [photo]`, `aiEdit <request>`, `importXmp`, `writeXmp`,
   `setZoom <scale|fit>`, `screenshot <file.png>`.

## Recipes

A recipe is a JSON object; omitted fields keep their defaults. The current one is
`recipe` in `getStatus` / `window_status`. Common fields:

| Field | Range | Default |
| :--- | :--- | :--- |
| `exposure` | -5 to +5 EV | 0 |
| `wb_temperature` | 2000 to 12000 K, relative to as shot | 5500 |
| `wb_tint`, `contrast`, `highlights`, `shadows`, `whites`, `blacks` | -100 to +100 | 0 |
| `texture`, `clarity`, `dehaze`, `vibrance`, `saturation` | -100 to +100 | 0 |
| `hsl_hue`, `hsl_sat`, `hsl_lum` | 8 values (red, orange, yellow, green, aqua, blue, purple, magenta) | 0 |
| `film_simulation` | for example `fuji_classic_chrome`, `fuji_acros_red`, `none` | `none` |
| `grain_amount`, `color_chrome`, `color_chrome_blue` | 0 to 100 | 0 |
| `sharpness`, `denoise_lum`, `denoise_col` | 0 to 100 | |
| `crop_x`, `crop_y`, `crop_w`, `crop_h` | 0 to 1 | whole frame |

To change one value in the window, read the recipe with `window_status`, change the
field and send it with `apply_recipe`, or use `adjust` for exposure and white balance.

## Export options

`format` (jxl, avif, webp, tiff, png, jpeg), `quality` (1-100), `scale_percent`,
`max_dimension`, `icc_profile` (sRGB, DisplayP3, AdobeRGB1998, ProPhotoRGB, Rec2020),
`rendering_intent` (perceptual, relative_colorimetric, saturation, absolute_colorimetric),
`output_dir` (default `~/Pictures/OmaStudio_Exports`), `strip_gps`, `preserve_exif`.

## Rules

- Original RAW files are never modified; adjustments live in `<photo>.omastudio` sidecars.
- Never delete or rewrite `~/.local/share/omastudio/catalog.json`, sidecars or the recipe
  library by hand; use the commands above.
- Writing a camera's custom setting slot (`fuji-camera write-preset`) replaces what is
  stored there: confirm the slot with the user first.
- Look at results: `render_preview` and `window_screenshot` return images.
