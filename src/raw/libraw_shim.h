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

/* Undemosaiced sensor data and what is needed to develop it (see omaraw_unpack_cfa). */
typedef struct {
    int raw_width;
    int raw_height;
    int raw_pitch;      /* row stride of the CFA data, in pixels */
    int top_margin;
    int left_margin;
    int width;          /* visible area */
    int height;
    unsigned filters;   /* Bayer pattern bits; 9 = X-Trans; 0 = not a CFA image */
    char xtrans[6][6];  /* X-Trans colours, indexed by visible-area row/col */
    int colors;
    int flip;           /* LibRaw sizes.flip: 0, 3 (180), 5 (90 CCW), 6 (90 CW) */
    float black[4];     /* per colour: black + cblack[c] */
    int black_pattern_h;
    int black_pattern_w;
    float black_pattern[36]; /* cblack[6..], indexed by visible row % h, col % w */
    float white;
    float cam_mul[4];   /* as-shot white balance */
    float pre_mul[4];   /* daylight white balance */
    float rgb_cam[3][4];/* camera RGB -> linear sRGB (D65), for white-balanced data */
} OmaRawCfaInfo;

void* omaraw_open(const char *path, int *errcode);
/* Unpacks the raw data and returns a pointer to the CFA plane (owned by the handle,
   valid until omaraw_close). Returns NULL when the file is not a plain 1-sample CFA
   (Foveon, linear DNG, Fuji SuperCCD, 4-colour sensors...); use omaraw_process_image then. */
const unsigned short* omaraw_unpack_cfa(void *handle, OmaRawCfaInfo *info);
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
