#ifndef OMARAW_LIBRAW_SHIM_H
#define OMARAW_LIBRAW_SHIM_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    int width;
    int height;
    int raw_width;
    int raw_height;
    char make[64];
    char model[64];
    char lens[128];
    float iso;
    float shutter;
    float aperture;
    float focal_len;
    long long timestamp;
    float cam_mul[4];
} OmaRawMetadata;

void* omaraw_open(const char *path, int *errcode);
int omaraw_get_metadata(void *handle, OmaRawMetadata *meta);
int omaraw_extract_thumb_file(void *handle, const char *dest_path);
/* output_color: LibRaw -o value (1 sRGB, 2 Adobe, 4 ProPhoto, 7 DCI-P3 D65, 8 Rec.2020);
   gamma_power / gamma_slope map to params.gamm[0] / gamm[1]. Call before processing. */
int omaraw_set_output_space(void *handle, int output_color, double gamma_power, double gamma_slope);
unsigned char* omaraw_process_image(void *handle, int half_size, int quality, int bps, int *out_w, int *out_h, int *out_colors, int *out_size);
void omaraw_free_image(unsigned char *ptr);
void omaraw_close(void *handle);

#ifdef __cplusplus
}
#endif

#endif // OMARAW_LIBRAW_SHIM_H
