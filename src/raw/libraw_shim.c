#include "libraw_shim.h"
#include <libraw/libraw.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

int omaraw_extract_thumb_file(void *handle, const char *dest_path) {
    if (!handle || !dest_path) return -1;
    libraw_data_t *lr = (libraw_data_t*)handle;

    int ret = libraw_unpack_thumb(lr);
    if (ret != LIBRAW_SUCCESS) {
        return ret;
    }

    int err = 0;
    libraw_processed_image_t *thumb = libraw_dcraw_make_mem_thumb(lr, &err);
    if (!thumb || err != 0) {
        return -2;
    }

    FILE *fp = fopen(dest_path, "wb");
    if (!fp) {
        libraw_dcraw_clear_mem(thumb);
        return -3;
    }

    if (thumb->type == LIBRAW_IMAGE_JPEG) {
        fwrite(thumb->data, 1, thumb->data_size, fp);
    } else if (thumb->type == LIBRAW_IMAGE_BITMAP) {
        // Write standard PPM header for bitmap thumb
        fprintf(fp, "P6\n%d %d\n255\n", thumb->width, thumb->height);
        fwrite(thumb->data, 1, thumb->data_size, fp);
    }

    fclose(fp);
    libraw_dcraw_clear_mem(thumb);
    return 0;
}

unsigned char* omaraw_process_image(void *handle, int half_size, int quality, int bps, int *out_w, int *out_h, int *out_colors, int *out_size) {
    if (!handle) return NULL;
    libraw_data_t *lr = (libraw_data_t*)handle;

    int ret = libraw_unpack(lr);
    if (ret != LIBRAW_SUCCESS) {
        return NULL;
    }

    // Configure Lightroom-grade raw extraction parameters
    lr->params.half_size = (half_size != 0) ? 1 : 0;
    lr->params.user_qual = quality; // 0=linear (fast), 3=AHD, 11=DHT, 12=AAHD
    lr->params.output_color = 1;    // 1 = sRGB
    lr->params.output_bps = (bps == 16) ? 16 : 8;
    lr->params.no_auto_bright = 1;  // Keep linear photonic brightness
    lr->params.use_camera_wb = 1;   // Use camera native white balance

    ret = libraw_dcraw_process(lr);
    if (ret != LIBRAW_SUCCESS) {
        return NULL;
    }

    int err = 0;
    libraw_processed_image_t *img = libraw_dcraw_make_mem_image(lr, &err);
    if (!img || err != 0) {
        return NULL;
    }

    if (out_w) *out_w = img->width;
    if (out_h) *out_h = img->height;
    if (out_colors) *out_colors = img->colors;
    if (out_size) *out_size = img->data_size;

    // Allocate continuous buffer to return to Rust with fail-closed safety
    unsigned char *buf = (unsigned char*)malloc(img->data_size);
    if (!buf) {
        libraw_dcraw_clear_mem(img);
        return NULL;
    }

    memcpy(buf, img->data, img->data_size);
    libraw_dcraw_clear_mem(img);
    return buf;
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
