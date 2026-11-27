#include "libraw_shim.h"
#include <libraw/libraw.h>
#include <limits.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int largest_thumb_index(libraw_data_t *lr);

void* omaraw_open(const char *path, int *errcode) {
    if (!path) {
        if (errcode) *errcode = -1;
        return NULL;
    }

    libraw_data_t *lr = libraw_init(0);
    if (!lr) {
        if (errcode) *errcode = -2;
        return NULL;
    }

    int ret = libraw_open_file(lr, path);
    if (ret != LIBRAW_SUCCESS) {
        if (errcode) *errcode = ret;
        libraw_close(lr);
        return NULL;
    }

    if (errcode) *errcode = 0;
    return (void*)lr;
}

int omaraw_get_metadata(void *handle, OmaRawMetadata *meta) {
    if (!handle || !meta) return -1;
    libraw_data_t *lr = (libraw_data_t*)handle;

    memset(meta, 0, sizeof(OmaRawMetadata));
    meta->width = lr->sizes.width;
    meta->height = lr->sizes.height;
    meta->raw_width = lr->sizes.raw_width;
    meta->raw_height = lr->sizes.raw_height;

    snprintf(meta->make, sizeof(meta->make), "%s", lr->idata.make);
    snprintf(meta->model, sizeof(meta->model), "%s", lr->idata.model);

    if (lr->lens.makernotes.Lens[0] != '\0') {
        snprintf(meta->lens, sizeof(meta->lens), "%s", lr->lens.makernotes.Lens);
    } else if (lr->lens.Lens[0] != '\0') {
        snprintf(meta->lens, sizeof(meta->lens), "%s", lr->lens.Lens);
    } else {
        snprintf(meta->lens, sizeof(meta->lens), "%s", "Standard Prime/Zoom");
    }

    meta->iso = lr->other.iso_speed;
    meta->shutter = lr->other.shutter;
    meta->aperture = lr->other.aperture;
    meta->focal_len = lr->other.focal_len;
    meta->timestamp = (long long)lr->other.timestamp;

    for (int i = 0; i < 4; i++) {
        meta->cam_mul[i] = lr->color.cam_mul[i];
    }

    return 0;
}

unsigned char* omaraw_extract_thumb_mem(void *handle, size_t *out_size) {
    if (out_size) *out_size = 0;
    if (!handle || !out_size) return NULL;
    libraw_data_t *lr = (libraw_data_t*)handle;

    int idx = largest_thumb_index(lr);
    int ret = idx >= 0 ? libraw_unpack_thumb_ex(lr, idx) : LIBRAW_UNSPECIFIED_ERROR;
    if (ret != LIBRAW_SUCCESS) ret = libraw_unpack_thumb(lr);
    if (ret != LIBRAW_SUCCESS) {
        return NULL;
    }

    int err = 0;
    libraw_processed_image_t *thumb = libraw_dcraw_make_mem_thumb(lr, &err);
    if (!thumb || err != 0 || thumb->data_size == 0) {
        if (thumb) libraw_dcraw_clear_mem(thumb);
        return NULL;
    }

    size_t data_size = (size_t)thumb->data_size;
    unsigned char *buf = NULL;
    if (thumb->type == LIBRAW_IMAGE_JPEG) {
        buf = (unsigned char*)malloc(data_size);
        if (buf) {
            memcpy(buf, thumb->data, data_size);
            *out_size = data_size;
        }
    } else if (thumb->type == LIBRAW_IMAGE_BITMAP) {
        if (thumb->width > 0 && thumb->height > 0) {
            char header[64];
            int hlen = snprintf(header, sizeof(header), "P6\n%u %u\n255\n", (unsigned)thumb->width, (unsigned)thumb->height);
            if (hlen > 0 && (size_t)hlen < sizeof(header) && SIZE_MAX - (size_t)hlen >= data_size) {
                size_t total = (size_t)hlen + data_size;
                buf = (unsigned char*)malloc(total);
                if (buf) {
                    memcpy(buf, header, (size_t)hlen);
                    memcpy(buf + (size_t)hlen, thumb->data, data_size);
                    *out_size = total;
                }
            }
        }
    }

    libraw_dcraw_clear_mem(thumb);
    return buf;
}

int omaraw_set_output_space(void *handle, int output_color, double gamma_power, double gamma_slope) {
    if (!handle) return -1;
    if (output_color < 1 || output_color > 8) return -1;
    libraw_data_t *lr = (libraw_data_t*)handle;
    lr->params.output_color = output_color;
    lr->params.gamm[0] = gamma_power;
    lr->params.gamm[1] = gamma_slope;
    return 0;
}

unsigned char* omaraw_process_image(void *handle, int half_size, int quality, int bps, int *out_w, int *out_h, int *out_colors, size_t *out_size) {
    if (out_w) *out_w = 0;
    if (out_h) *out_h = 0;
    if (out_colors) *out_colors = 0;
    if (out_size) *out_size = 0;
    if (!handle) return NULL;
    libraw_data_t *lr = (libraw_data_t*)handle;

    // omaraw_unpack_cfa may already have unpacked this handle; unpacking again
    // fails for some decoders (Fujifilm compressed RAF), so only unpack once
    int ret;
    if (!(lr->progress_flags & LIBRAW_PROGRESS_LOAD_RAW)) {
        ret = libraw_unpack(lr);
        if (ret != LIBRAW_SUCCESS) {
            return NULL;
        }
    }

    // Configure Lightroom-grade raw extraction parameters
    lr->params.half_size = (half_size != 0) ? 1 : 0;
    lr->params.user_qual = quality; // 0=linear (fast), 3=AHD, 11=DHT, 12=AAHD
    // output_color: LibRaw defaults to 1 (sRGB); omaraw_set_output_space() may pick a
    // wide-gamut space for export, so it is not overridden here.
    lr->params.output_bps = (bps == 16) ? 16 : 8;
    lr->params.no_auto_bright = 1;  // No auto exposure stretch (output stays BT.709 gamma-encoded: default gamm 0.45/4.5)
    lr->params.use_camera_wb = 1;   // Use camera native white balance

    ret = libraw_dcraw_process(lr);
    if (ret != LIBRAW_SUCCESS) {
        return NULL;
    }

    int err = 0;
    libraw_processed_image_t *img = libraw_dcraw_make_mem_image(lr, &err);
    if (!img || err != 0 || img->data_size == 0 || img->width == 0 || img->height == 0 || img->colors == 0) {
        if (img) libraw_dcraw_clear_mem(img);
        return NULL;
    }

    size_t data_size = (size_t)img->data_size;
    // Allocate continuous buffer to return to Rust with fail-closed safety
    unsigned char *buf = (unsigned char*)malloc(data_size);
    if (!buf) {
        libraw_dcraw_clear_mem(img);
        return NULL;
    }

    memcpy(buf, img->data, data_size);
    if (out_w) *out_w = (int)img->width;
    if (out_h) *out_h = (int)img->height;
    if (out_colors) *out_colors = (int)img->colors;
    if (out_size) *out_size = data_size;
    libraw_dcraw_clear_mem(img);
    return buf;
}

// Phase One / Leaf IIQ: the raw plane still carries the back's per-row and per-column
// black levels, and the sensor calibration (flat field, defects, split-sensor gains)
// lives in the file. LibRaw applies both only while it copies the plane into image[].
static int is_phase_one_decoder(libraw_data_t *lr) {
    libraw_decoder_info_t d;
    if (libraw_get_decoder_info(lr, &d) != LIBRAW_SUCCESS || !d.decoder_name) return 0;
    return strncmp(d.decoder_name, "phase_one_load_raw", 18) == 0;
}

// Runs LibRaw's raw2image (black subtraction + Phase One correction) and packs the
// visible area back into a single CFA plane, in place in image[]. The plane stays
// valid until the handle is processed again or closed.
static const unsigned short* phase_one_cfa(libraw_data_t *lr, OmaRawCfaInfo *info) {
    lr->params.half_size = 0;
    if (libraw_raw2image(lr) != LIBRAW_SUCCESS || !lr->image) return NULL;
    int w = lr->sizes.iwidth, h = lr->sizes.iheight;
    if (w <= 0 || h <= 0 || w != lr->sizes.width || h != lr->sizes.height) return NULL;

    unsigned short (*img)[4] = lr->image;
    unsigned short *plane = (unsigned short*)lr->image;
    // Writing index i reads index 4*i, so packing from the front never overwrites
    // a pixel that is still to be read.
    for (int row = 0; row < h; row++)
        for (int col = 0; col < w; col++) {
            size_t i = (size_t)row * (size_t)w + (size_t)col;
            plane[i] = img[i][libraw_COLOR(lr, row, col)];
        }

    info->raw_width = w;
    info->raw_height = h;
    info->raw_pitch = w;
    info->top_margin = 0;
    info->left_margin = 0;
    // phase_one_subtract_black removed the whole black level
    for (int c = 0; c < 4; c++) info->black[c] = 0.0f;
    info->black_pattern_h = 0;
    info->black_pattern_w = 0;
    return plane;
}

// Sizes, levels and colour data shared by mosaic and linear sensors. Returns 0 when
// the layout is not one the developer can read safely.
static int fill_info(libraw_data_t *lr, OmaRawCfaInfo *info) {
    memset(info, 0, sizeof(OmaRawCfaInfo));

    uint64_t raw_w = (uint64_t)lr->sizes.raw_width;
    uint64_t raw_h = (uint64_t)lr->sizes.raw_height;
    uint64_t vis_w = (uint64_t)lr->sizes.width;
    uint64_t vis_h = (uint64_t)lr->sizes.height;
    uint64_t top = (uint64_t)lr->sizes.top_margin;
    uint64_t left = (uint64_t)lr->sizes.left_margin;
    uint64_t pitch = (uint64_t)(lr->sizes.raw_pitch / 2);

    if (raw_w == 0 || raw_h == 0 || vis_w == 0 || vis_h == 0) return 0;
    if (raw_w > (uint64_t)INT_MAX || raw_h > (uint64_t)INT_MAX) return 0;
    if (vis_w > (uint64_t)INT_MAX || vis_h > (uint64_t)INT_MAX) return 0;
    if (top > (uint64_t)INT_MAX || left > (uint64_t)INT_MAX || pitch > (uint64_t)INT_MAX) return 0;
    if (pitch < raw_w) return 0;
    if (top + vis_h > raw_h) return 0;
    if (left + vis_w > raw_w) return 0;

    info->raw_width = (int)raw_w;
    info->raw_height = (int)raw_h;
    info->raw_pitch = (int)pitch;
    info->top_margin = (int)top;
    info->left_margin = (int)left;
    info->width = (int)vis_w;
    info->height = (int)vis_h;
    info->filters = lr->idata.filters;
    memcpy(info->xtrans, lr->idata.xtrans, sizeof(info->xtrans));
    info->colors = lr->idata.colors;
    info->flip = lr->sizes.flip;

    for (int c = 0; c < 4; c++) {
        info->black[c] = (float)(lr->color.black + lr->color.cblack[c]);
        info->cam_mul[c] = lr->color.cam_mul[c];
        info->pre_mul[c] = lr->color.pre_mul[c];
    }
    unsigned ph = lr->color.cblack[4], pw = lr->color.cblack[5];
    if (ph && pw) {
        if (ph > 36 || pw > 36 || (uint64_t)ph * (uint64_t)pw > 36) return 0;
        unsigned count = ph * pw;
        info->black_pattern_h = (int)ph;
        info->black_pattern_w = (int)pw;
        for (unsigned i = 0; i < count; i++) info->black_pattern[i] = (float)lr->color.cblack[6 + i];
    }
    info->white = (float)lr->color.maximum;
    memcpy(info->rgb_cam, lr->color.rgb_cam, sizeof(info->rgb_cam));

    return 1;
}

const unsigned short* omaraw_unpack_cfa(void *handle, OmaRawCfaInfo *info) {
    if (!handle || !info) return NULL;
    libraw_data_t *lr = (libraw_data_t*)handle;

    if (!(lr->progress_flags & LIBRAW_PROGRESS_LOAD_RAW) && libraw_unpack(lr) != LIBRAW_SUCCESS)
        return NULL;
    if (!lr->rawdata.raw_image) return NULL;
    if (lr->idata.filters == 0 || lr->idata.colors != 3) return NULL;
    if (lr->rawdata.ioparams.fuji_width != 0) return NULL;
    if (lr->sizes.pixel_aspect < 0.999 || lr->sizes.pixel_aspect > 1.001) return NULL;

    if (!fill_info(lr, info)) return NULL;

    if (is_phase_one_decoder(lr))
        return phase_one_cfa(lr, info);
    return lr->rawdata.raw_image;
}

const unsigned short* omaraw_unpack_linear(void *handle, OmaRawCfaInfo *info, int *stride) {
    if (!handle || !info || !stride) return NULL;
    libraw_data_t *lr = (libraw_data_t*)handle;

    if (!(lr->progress_flags & LIBRAW_PROGRESS_LOAD_RAW) && libraw_unpack(lr) != LIBRAW_SUCCESS)
        return NULL;
    if (lr->idata.filters != 0) return NULL;
    if (lr->rawdata.ioparams.fuji_width != 0) return NULL;
    if (lr->sizes.pixel_aspect < 0.999 || lr->sizes.pixel_aspect > 1.001) return NULL;
    if (!fill_info(lr, info)) return NULL;

    const unsigned short *plane = NULL;
    int n = 0;
    if (lr->idata.colors == 1 && lr->rawdata.raw_image) {
        plane = lr->rawdata.raw_image;
        n = 1;
    } else if (lr->idata.colors == 3 && lr->rawdata.color3_image) {
        plane = (const unsigned short*)lr->rawdata.color3_image;
        n = 3;
    } else if (lr->idata.colors == 3 && lr->rawdata.color4_image) {
        plane = (const unsigned short*)lr->rawdata.color4_image;
        n = 4;
    }
    if (!plane) return NULL;

    // raw_pitch is in bytes; one pixel holds n samples
    uint64_t linear_pitch = (uint64_t)lr->sizes.raw_pitch / (uint64_t)(2 * n);
    if (linear_pitch > (uint64_t)INT_MAX || linear_pitch < (uint64_t)info->raw_width) return NULL;
    info->raw_pitch = (int)linear_pitch;
    *stride = n;
    return plane;
}

/* The largest embedded preview: medium format files carry several (a small EXIF
   thumbnail and a full-size JPEG), and LibRaw's default pick is not always the biggest. */
static int largest_thumb_index(libraw_data_t *lr) {
    int best = -1;
    long long best_area = 0;
    for (int i = 0; i < lr->thumbs_list.thumbcount && i < LIBRAW_THUMBNAIL_MAXCOUNT; i++) {
        libraw_thumbnail_item_t *t = &lr->thumbs_list.thumblist[i];
        if (t->tformat != LIBRAW_INTERNAL_THUMBNAIL_JPEG) continue;
        long long area = (long long)t->twidth * t->theight;
        if (area > best_area) {
            best_area = area;
            best = i;
        }
    }
    return best;
}

int omaraw_camera_count(void) {
    return libraw_cameraCount();
}

const char* omaraw_camera_name(int index) {
    if (index < 0 || index >= libraw_cameraCount()) return NULL;
    return libraw_cameraList()[index];
}

void omaraw_free_image(unsigned char *ptr) {
    if (ptr) {
        free(ptr);
    }
}

void omaraw_close(void *handle) {
    if (handle) {
        libraw_close((libraw_data_t*)handle);
    }
}
