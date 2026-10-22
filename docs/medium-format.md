# Medium format support

OmaStudio opens raws from Hasselblad (3FR, FFF), Phase One and Leaf (IIQ, MOS),
Mamiya (MEF) and Pentax 645 (PEF/DNG). LibRaw decodes the sensor data. The code in
`src/medium_format/` reads what the makers' own tools (Phocus and Capture One) read
next to the sensor data, and adds what those tools do on top of it.

## Clean-room rule

These notes describe file formats and a USB protocol so that OmaStudio can work
with the files and cameras. They were worked out from three sources: the sample
files listed below, the public DNG and TIFF specifications, and how Phocus 4.2.3
for macOS behaves (its symbol names, its string tables, and the order in which it
calls the system's USB functions).

No Hasselblad, Phase One or LibRaw code is copied into this repository. The same
goes for any data shipped inside Phocus or Capture One: HNCS colour tables, the
encrypted lens and colour maps (`GMaps`, `Colormaps` `.dat` files), and firmware.
LibRaw and Lensfun are only used as libraries and as reference.

## Hasselblad 3FR and FFF

The camera writes a 3FR. Phocus writes an FFF when it imports one, and the layout
is the same.

- The file is a TIFF container. The CFA raw sits in a SubIFD whose
  PhotometricInterpretation is 32803 (CFA). Older backs keep it in IFD0.
- The raw IFD carries the DNG tags `BlackLevel` (0xc61a), `WhiteLevel` (0xc61d),
  `DefaultCropOrigin` (0xc61f), `DefaultCropSize` (0xc620) and `MaskedAreas` (0xc68e).
- IFD0 carries `ColorMatrix1` (0xc621), `AsShotNeutral` (0xc628),
  `BaselineSharpness` (0xc62c) and `UniqueCameraModel` (0xc614).
- The EXIF MakerNote is a plain IFD, and its value offsets count from the start of
  the file:

| Tag    | Meaning                                                  |
|--------|----------------------------------------------------------|
| 0x0015 | Model name, for example "X2D 100C"                       |
| 0x0019 | 512-byte sensor gain header (starts with "GAIN")         |
| 0x0042 | 1024-byte histogram                                      |
| 0x0059 | `[1, x, y, width, height]`, the same crop as DefaultCrop |
| 0x005a | 4096 bytes of encrypted calibration data (not read)      |
| 0x0060, 0x0061 | Serial strings, meaning unknown                  |

LibRaw's visible area still includes the dark border photosites. OmaStudio's own
develop path crops to DefaultCrop and re-bases the Bayer pattern when the crop
origin is odd. Measured crops:

| Camera  | Crop origin | Crop size  | Raw plane  |
|---------|-------------|------------|------------|
| X2D 100C | 128, 96    | 11656×8742 | 11904×8842 |
| X1D II 50C | 50, 100  | 8272×6200  | 8384×6304  |
| H6D-400c | 68, 112    | 11600×8700 | 12000×8816 |

## Phase One and Leaf IIQ

An IIQ starts with a TIFF header. At byte 8 come "IIII" and a "Raw" magic word.
The word at byte 16 points to Phase One's own directory, with offsets counting from
byte 8. The directory holds an entry count and a reserved word, followed by 16-byte
entries (tag, type, length, value or offset).

| Tag    | Meaning                                         |
|--------|-------------------------------------------------|
| 0x0102 | Back serial number                              |
| 0x0105 | ISO                                             |
| 0x0108, 0x0109 | Raw width and height                    |
| 0x010a, 0x010b | Left and top of the image area          |
| 0x010c, 0x010d | Image area width and height             |
| 0x010e | Raw format (compression and bit depth)          |
| 0x0210 | Sensor temperature (f32 bits, °C)               |
| 0x0301 | Back model and firmware, "IQ4 150MP, v7.00.7 (FWP)" |
| 0x0410 | Camera body                                     |
| 0x0412 | Lens                                            |
| 0x0455 | Viewfinder                                      |

The IIQ sensor calibration (per-column black levels, defect map, flat field and
split-sensor gains) is applied by LibRaw only while it copies the raw plane into
its working image. For Phase One decoders, the shim (`src/raw/libraw_shim.c`)
therefore runs `raw2image` and packs the corrected plane back into one CFA plane.
Without that step, IQ4 files came out magenta.

## Lens corrections

Phocus corrects XCD and HC lenses with data that ships inside the application in
encrypted form. That data is not used here. OmaStudio offers two open routes
instead:

1. **Flat field (LCC).** Shoot an evenly lit white target (an LCC card or a
   diffuser) with the same lens, aperture and shift as the shoot. Then run
   `omastudio-engine medium-format lcc-create <shot>`. This writes `lcc.json` next
   to the shot, and every medium format raw in that folder is corrected with it.
   The result is a 48×36 gain grid in sensor coordinates, so it follows rotated
   frames. A per-file `<raw>.lcc.json` takes precedence over the folder file.
2. **Lens profiles.** Lensfun XML files are read from the system Lensfun database,
   from `~/.local/share/lensfun` and from `~/.local/share/omastudio/lenses`. The
   database has Schneider LS, Pentax 645 and Mamiya lenses, but no XCD or HC lenses.
   `lcc-create --lensfun` fits a PA vignetting model to the flat field and writes a
   profile for the recorded lens into the last folder.

Corrections only run for medium format extensions. Set
`OMASTUDIO_LENS_CORRECTION=off` to turn them off.

## Multi-shot

Pixel-shift backs take four frames, moving the sensor by one photosite between
frames in the order (0,0), (0,1), (1,1), (1,0). Every scene point is then seen
through red, blue and two green filters, so no demosaicing is needed. Run
`medium-format multishot f1 f2 f3 f4 --out merged.tif` to merge four frames into a
16-bit TIFF. The 6- and 16-shot modes add half-pixel steps on top of this. They are
not handled yet because no sample files were available.

## Hasselblad tethering (experimental)

X and V system cameras (X1D, X2D, 907X/CFV II) do not speak PTP. Phocus opens a
vendor-specific USB interface (class 0xff) and exchanges small "hblm" signal
messages with the processors inside the camera.

- USB vendor ids: 0x2756 (Victor Hasselblad AB) and 0x2ca3 (DJI).
- Protocol generation: product ids 7 to 30 under 0x2756 use v2, all others use v1.
- Nodes: 0 none, 1 main processor ("farm"), 2 power control, 3 system controller
  ("suc"), 4 host, 5 image processor.
- A signal id is an index into the camera's signal table (about 790 entries). The
  ids used here:

| Id    | Signal                                     | Payload |
|-------|--------------------------------------------|---------|
| 0x000b / 0x000c | System controller firmware request / response | 1 / 128 |
| 0x000d / 0x000e | Main processor firmware request / response    | 1 / 192 |
| 0x002c | Exposure started (event)                  | 1       |
| 0x002f | Exposure done (event)                     | 1       |
| 0x00c4 / 0x00c5 | Release the shutter / response    | 1 / 1   |
| 0x0201 / 0x0202 | Exposures left / response         | 1 / 4   |
| 0x02ed / 0x02ee | Get ISO / response                | 1 / 4   |
| 0x02ef / 0x02f0 | Set ISO / response                | 4 / 1   |

Frame layouts:

```
v2:  [signal u16 LE][src u8][dst u8][len u8][payload]
     len = 0 → [len u16 BE] follows at offset 5 and the payload starts at 7
v1:  first fragment:  [signal][src][dst][len = size−5][index u32 = 0][msg id u8][total u32][≤246 bytes]
     next fragments:  [signal][src][dst][len][index u32][msg id u8][≤250 bytes]
```

How these frames are packed into bulk transfers has not been confirmed on a camera
yet. Before the commands become a normal feature, a `usbmon` capture of Phocus
talking to a camera is needed to check that. Until then, `medium-format tether`
needs `--experimental` (except for `list`). The udev rule
`packaging/udev/70-omastudio-cameras.rules` gives the logged-in user access to the
vendor interface.

## Sample files

All samples come from raw.pixls.us: X2D 100C (3FR and FFF), X1D II 50C,
CFV II 50C, H6D-400c, H5D-40, H3D, CF132, IQ4 150MP, IQ3 100MP, P45+, Leaf Credo 40,
Leaf Aptus 22 (MOS), Mamiya ZD (MEF) and Pentax 645Z (PEF). Every one of them opens.
Known gaps:

- H-series lens names are missing from LibRaw's metadata.
- The Aptus 22 MOS has no EXIF, so its ISO reads as 0.
- The Mamiya ZD has no as-shot white balance, so its white balance falls back to
  daylight.
