//! CUDA kernel sources used by the M3 GPU pipeline.
//!
//! Kept as static strings rather than .cu files so cargo doesn't need a
//! build-time CUDA toolchain — NVRTC compiles them at runtime via
//! `cudarc::nvrtc::compile_ptx`.

/// RGBA8 → NV12 conversion (BT.601 limited range).
///
/// One thread handles a 2x2 RGBA block ⇒ writes 4 Y samples and 1
/// interleaved UV pair (NV12 has half-resolution chroma in 4:2:0).
///
/// Matrix:
///   Y =  0.257*R + 0.504*G + 0.098*B + 16
///   U = -0.148*R - 0.291*G + 0.439*B + 128
///   V =  0.439*R - 0.368*G - 0.071*B + 128
///
/// Coefficients chosen for TV-range BT.601 because that's what
/// h264_nvenc defaults to when not given an explicit color_range hint.
/// We also clamp into [16, 235] / [16, 240] to avoid out-of-range
/// values surviving through to the encoder.
///
/// Args (in order):
///   const uint8_t* rgba       device pointer to W*H*4 bytes (row-major,
///                             tightly packed; the ffmpeg frame copy
///                             handles linesize)
///   uint8_t*       y_plane    device pointer to W*H bytes
///   uint8_t*       uv_plane   device pointer to W*(H/2) bytes (interleaved UV)
///   int            width
///   int            height
///   int            rgba_pitch row stride for rgba (bytes), 0 = use W*4
///   int            y_pitch    row stride for y_plane (bytes), 0 = use W
///   int            uv_pitch   row stride for uv_plane (bytes), 0 = use W
pub const RGBA_TO_NV12: &str = r#"
extern "C" __global__ void rgba_to_nv12(
    const unsigned char* __restrict__ rgba,
    unsigned char* __restrict__ y_plane,
    unsigned char* __restrict__ uv_plane,
    int width, int height,
    int rgba_pitch, int y_pitch, int uv_pitch
) {
    int x2 = (blockIdx.x * blockDim.x + threadIdx.x) * 2; // top-left x of 2x2 block
    int y2 = (blockIdx.y * blockDim.y + threadIdx.y) * 2; // top-left y of 2x2 block
    if (x2 >= width || y2 >= height) return;

    const int rp = rgba_pitch ? rgba_pitch : (width * 4);
    const int yp = y_pitch    ? y_pitch    : width;
    const int uvp = uv_pitch  ? uv_pitch   : width;

    float r_sum = 0.0f, g_sum = 0.0f, b_sum = 0.0f;
    int n = 0;

    for (int dy = 0; dy < 2; ++dy) {
        int yy = y2 + dy;
        if (yy >= height) continue;
        for (int dx = 0; dx < 2; ++dx) {
            int xx = x2 + dx;
            if (xx >= width) continue;
            const unsigned char* p = rgba + yy * rp + xx * 4;
            float r = (float)p[0];
            float g = (float)p[1];
            float b = (float)p[2];

            // Per-pixel Y (full resolution).
            float y = 0.257f * r + 0.504f * g + 0.098f * b + 16.0f;
            int yi = (int)(y + 0.5f);
            if (yi < 16)  yi = 16;
            if (yi > 235) yi = 235;
            y_plane[yy * yp + xx] = (unsigned char)yi;

            // Accumulate for chroma subsampling.
            r_sum += r; g_sum += g; b_sum += b;
            n++;
        }
    }

    if (n > 0) {
        float r_avg = r_sum / (float)n;
        float g_avg = g_sum / (float)n;
        float b_avg = b_sum / (float)n;

        float u = -0.148f * r_avg - 0.291f * g_avg + 0.439f * b_avg + 128.0f;
        float v =  0.439f * r_avg - 0.368f * g_avg - 0.071f * b_avg + 128.0f;
        int ui = (int)(u + 0.5f);
        int vi = (int)(v + 0.5f);
        if (ui < 16)  ui = 16;
        if (ui > 240) ui = 240;
        if (vi < 16)  vi = 16;
        if (vi > 240) vi = 240;

        // NV12 UV plane is half-height of Y; UV pairs are interleaved (U,V,U,V,...).
        int uv_row = y2 / 2;
        int uv_col = (x2 / 2) * 2; // each UV pair takes 2 bytes
        uv_plane[uv_row * uvp + uv_col + 0] = (unsigned char)ui;
        uv_plane[uv_row * uvp + uv_col + 1] = (unsigned char)vi;
    }
}
"#;
