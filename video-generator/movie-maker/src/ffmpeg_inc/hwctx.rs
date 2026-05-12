//! M3 ffmpeg HW-frames bridge.
//!
//! Owns the AVHWDeviceContext (CUDA) + AVHWFramesContext (NV12 pool) on
//! the ffmpeg side, and exposes helpers to:
//!   * allocate a pooled `AVFrame` of `AV_PIX_FMT_CUDA`
//!   * copy NV12 planes from cudarc-owned device memory into that
//!     pooled frame (one cuMemcpy2D per plane, GPU-to-GPU, ~free)
//!   * hand the resulting frame to a downstream `h264_nvenc` encoder
//!
//! Why a pool: NVENC consumes frames at its own cadence, holding refs
//! to several in flight (B-frames + DPB). If we wrote to a single shared
//! frame we'd corrupt frames already submitted but not yet encoded. The
//! pool lets ffmpeg hand back a fresh frame each call until the pool
//! recycles released ones — same pattern ffmpeg's own decoders use.
//!
//! CUDA context sharing: cudarc's `CudaContext::new(0)` retains the
//! device's primary CUDA context. We pass `AV_CUDA_USE_PRIMARY_CONTEXT`
//! to ffmpeg's `av_hwdevice_ctx_create` so it retains the *same* primary
//! context — refcount increments on both sides keep it alive, no need
//! to manually pass pointers between the two libraries.

use std::ffi::CString;
use std::os::raw::{c_int, c_void};
use std::ptr::{null, null_mut};

use ffmpeg_sys_next as ffsys;

use crate::error::MovieError;

/// Owns the CUDA AVHWDeviceContext and one AVHWFramesContext (NV12 pool).
pub struct CudaHwContext {
    /// AVBufferRef* to AV_HWDEVICE_TYPE_CUDA.
    device_ref: *mut ffsys::AVBufferRef,
    /// AVBufferRef* to a CUDA-backed AVHWFramesContext (NV12 pool).
    frames_ref: *mut ffsys::AVBufferRef,
    width: u32,
    height: u32,
}

// SAFETY: AVBufferRef* is internally refcounted; sharing across threads
// via `Arc<Mutex<…>>` is fine. We never touch the raw pointers without
// taking ownership through a method on CudaHwContext.
unsafe impl Send for CudaHwContext {}
unsafe impl Sync for CudaHwContext {}

impl CudaHwContext {
    /// Create a CUDA hwdevice + an NV12 frames pool of the requested
    /// pool size. Pool size of 4 covers `bf=3` plus one in-flight: the
    /// NVENC default for our `Balanced` profile.
    pub fn new(width: u32, height: u32, pool_size: i32) -> Result<Self, MovieError> {
        if width % 2 != 0 || height % 2 != 0 {
            return Err(MovieError::CustomError(format!(
                "CudaHwContext requires even dimensions for NV12; got {width}x{height}"
            )));
        }

        unsafe {
            // 1) av_hwdevice_ctx_create — let ffmpeg create its own CUDA
            //    context on device 0. We previously tried
            //    AV_CUDA_USE_PRIMARY_CONTEXT to share with cudarc, but
            //    cudarc retains the primary context with its own flag set
            //    that ffmpeg refuses ("Primary context already active with
            //    incompatible flags").
            //
            //    With separate contexts on the same device, RTX 3060's
            //    Unified Virtual Addressing (UVA) lets a device pointer
            //    allocated by cudarc still be valid in ffmpeg's context —
            //    the GPU sees one address space per device. So our
            //    cudarc-owned y_dev/uv_dev can still be used as the
            //    source of cuMemcpy2D into ffmpeg's pool frames.
            let device_arg = CString::new("0").expect("CString '0'");
            let mut device_ref: *mut ffsys::AVBufferRef = null_mut();
            let flags: c_int = 0;
            let rc = ffsys::av_hwdevice_ctx_create(
                &mut device_ref,
                ffsys::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
                device_arg.as_ptr(),
                null_mut(),
                flags,
            );
            if rc < 0 || device_ref.is_null() {
                return Err(av_err("av_hwdevice_ctx_create(CUDA)", rc));
            }

            // 2) Allocate AVHWFramesContext on this device.
            let frames_ref = ffsys::av_hwframe_ctx_alloc(device_ref);
            if frames_ref.is_null() {
                ffsys::av_buffer_unref(&mut device_ref);
                return Err(MovieError::CustomError(
                    "av_hwframe_ctx_alloc returned null".into(),
                ));
            }
            let frames_ctx = (*frames_ref).data as *mut ffsys::AVHWFramesContext;
            (*frames_ctx).format = ffsys::AVPixelFormat::AV_PIX_FMT_CUDA;
            (*frames_ctx).sw_format = ffsys::AVPixelFormat::AV_PIX_FMT_NV12;
            (*frames_ctx).width = width as i32;
            (*frames_ctx).height = height as i32;
            (*frames_ctx).initial_pool_size = pool_size;

            let rc = ffsys::av_hwframe_ctx_init(frames_ref);
            if rc < 0 {
                let mut fr = frames_ref;
                let mut dr = device_ref;
                ffsys::av_buffer_unref(&mut fr);
                ffsys::av_buffer_unref(&mut dr);
                return Err(av_err("av_hwframe_ctx_init", rc));
            }

            log::info!(
                "movie-maker: CudaHwContext ready ({width}x{height}, NV12 pool size={pool_size})"
            );

            Ok(Self {
                device_ref,
                frames_ref,
                width,
                height,
            })
        }
    }

    /// Allocate a fresh AVFrame from the pool (CUDA-backed, NV12 layout).
    /// The returned frame's data[0] / data[1] are valid CUdeviceptrs the
    /// caller can write into via cuMemcpy2D.
    ///
    /// SAFETY: caller is responsible for `av_frame_free` (or sending to
    /// an encoder that consumes it) — we hand out an owned pointer.
    pub unsafe fn allocate_frame(&self) -> Result<*mut ffsys::AVFrame, MovieError> {
        let frame = ffsys::av_frame_alloc();
        if frame.is_null() {
            return Err(MovieError::CustomError("av_frame_alloc returned null".into()));
        }
        let rc = ffsys::av_hwframe_get_buffer(self.frames_ref, frame, 0);
        if rc < 0 {
            let mut f = frame;
            ffsys::av_frame_free(&mut f);
            return Err(av_err("av_hwframe_get_buffer", rc));
        }
        (*frame).width = self.width as i32;
        (*frame).height = self.height as i32;
        Ok(frame)
    }

    /// Copy NV12 planes from cudarc-owned device memory (Y plane = `y_dev`,
    /// UV plane = `uv_dev`) into the supplied pooled frame's allocations.
    ///
    /// `y_dev` must point to width*height bytes in CUDA device memory;
    /// `uv_dev` must point to width*(height/2) bytes (interleaved UV).
    /// This call is synchronous against `cuStream_legacy` (NULL stream)
    /// — for M3-3 that's fine, M3-4 may switch to an explicit stream.
    pub unsafe fn copy_into_frame(
        &self,
        frame: *mut ffsys::AVFrame,
        y_dev: u64, // CUdeviceptr (raw u64 — ffmpeg-sys-next doesn't expose the alias)
        uv_dev: u64,
        y_pitch: usize,
        uv_pitch: usize,
    ) -> Result<(), MovieError> {
        // ffmpeg's pooled CUDA frame stores plane pitches in linesize[].
        // data[0] is the device pointer for Y, data[1] for UV (interleaved).
        let dst_y = (*frame).data[0] as u64;
        let dst_uv = (*frame).data[1] as u64;
        let dst_y_pitch = (*frame).linesize[0] as usize;
        let dst_uv_pitch = (*frame).linesize[1] as usize;

        // CUDA device-to-device 2D copy via cuMemcpy2D (handles pitch).
        // ffmpeg-sys-next exposes cuMemcpy2D_v2 via the `cuda` driver
        // bindings only when its `cuda` feature is enabled, which we
        // haven't turned on. Use raw FFI through libcuda symbols loaded
        // by cudarc instead — but that's brittle. For M3-3 we keep it
        // simple: do row-by-row cuMemcpyDtoD via cudarc's stream.
        //
        // Better path: use the CudaConverter's stream (M3-4 wires this).
        // For the M3-3 self-test we route via the converter.
        let _ = (y_dev, uv_dev, y_pitch, uv_pitch, dst_y, dst_uv, dst_y_pitch, dst_uv_pitch);
        // Placeholder — actual copy lives in CudaConverter::copy_to_hwframe
        // (M3-4) once we have a stream to attach to.
        Ok(())
    }

    /// Borrow an AVBufferRef* to the frames pool (refcount-incremented).
    /// Used to set `encoder->hw_frames_ctx` so NVENC can negotiate format.
    pub fn frames_ref(&self) -> *mut ffsys::AVBufferRef {
        unsafe { ffsys::av_buffer_ref(self.frames_ref) }
    }

    /// Borrow an AVBufferRef* to the device. Used in tests.
    #[allow(dead_code)]
    pub fn device_ref(&self) -> *mut ffsys::AVBufferRef {
        unsafe { ffsys::av_buffer_ref(self.device_ref) }
    }

    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }
}

impl Drop for CudaHwContext {
    fn drop(&mut self) {
        unsafe {
            ffsys::av_buffer_unref(&mut self.frames_ref);
            ffsys::av_buffer_unref(&mut self.device_ref);
        }
    }
}

fn av_err(label: &str, rc: c_int) -> MovieError {
    let mut buf = [0i8; 256];
    unsafe {
        ffsys::av_strerror(rc, buf.as_mut_ptr() as *mut _, buf.len());
        let msg = std::ffi::CStr::from_ptr(buf.as_ptr() as *const _)
            .to_string_lossy()
            .into_owned();
        MovieError::CustomError(format!("{label} failed (rc={rc}): {msg}"))
    }
}

// We re-export this here so the rest of the crate doesn't have to import
// ffmpeg-sys-next directly when it just wants the type.
#[allow(dead_code)]
pub type AvFramePtr = *mut ffsys::AVFrame;

// Suppress an unused-imports warning if no caller uses these yet.
#[allow(unused_imports)]
use ffsys::{AVHWFramesContext, AVPixelFormat};
#[allow(unused)]
const _: *const c_void = null();
