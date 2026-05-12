//! M3 GPU pipeline: RGBA8 (CPU) → NV12 (GPU) → NVENC.
//!
//! This module owns the cudarc-side state (CudaContext, compiled module,
//! kernel function handle, persistent device buffers). The ffmpeg
//! hwframe interop is handled in `crate::ffmpeg_inc::hwctx` (M3-3).

pub mod kernels;

use cudarc::driver::{
    CudaContext, CudaFunction, CudaSlice, CudaStream, LaunchConfig, PushKernelArg,
};
use std::sync::Arc;

use crate::error::MovieError;

/// All persistent GPU resources for the RGBA→NV12 conversion of a
/// fixed-size frame stream. Owns one CUDA context, one stream, the
/// compiled kernel, and pre-allocated device buffers (RGBA + NV12
/// planes) reused across frames — so the per-frame cost is only the
/// upload + launch + (downstream) NVENC submit, no allocation.
pub struct CudaConverter {
    ctx: Arc<CudaContext>,
    stream: Arc<CudaStream>,
    kernel: CudaFunction,

    width: u32,
    height: u32,

    rgba_dev: CudaSlice<u8>,
    y_dev: CudaSlice<u8>,
    uv_dev: CudaSlice<u8>,
}

impl CudaConverter {
    /// Initialize the converter for a given frame size. Compiles the
    /// kernel via NVRTC on first call (the CudaContext lives for the
    /// rest of the process).
    pub fn new(width: u32, height: u32) -> Result<Self, MovieError> {
        if width % 2 != 0 || height % 2 != 0 {
            return Err(MovieError::CustomError(format!(
                "CudaConverter requires even dimensions for NV12 4:2:0 chroma; got {width}x{height}"
            )));
        }
        let ctx = CudaContext::new(0).map_err(|e| {
            MovieError::CustomError(format!("CudaContext::new(0) failed: {e:?}"))
        })?;
        let stream = ctx.default_stream();

        let ptx = cudarc::nvrtc::compile_ptx(kernels::RGBA_TO_NV12).map_err(|e| {
            MovieError::CustomError(format!("nvrtc compile of rgba_to_nv12 failed: {e:?}"))
        })?;
        let module = ctx.load_module(ptx).map_err(|e| {
            MovieError::CustomError(format!("CUDA module load failed: {e:?}"))
        })?;
        let kernel = module.load_function("rgba_to_nv12").map_err(|e| {
            MovieError::CustomError(format!("load rgba_to_nv12: {e:?}"))
        })?;

        // Persistent device buffers, allocated once.
        let rgba_bytes = (width as usize) * (height as usize) * 4;
        let y_bytes = (width as usize) * (height as usize);
        let uv_bytes = (width as usize) * ((height as usize) / 2);

        let rgba_dev = stream.alloc_zeros::<u8>(rgba_bytes).map_err(map_err)?;
        let y_dev = stream.alloc_zeros::<u8>(y_bytes).map_err(map_err)?;
        let uv_dev = stream.alloc_zeros::<u8>(uv_bytes).map_err(map_err)?;

        Ok(Self {
            ctx,
            stream,
            kernel,
            width,
            height,
            rgba_dev,
            y_dev,
            uv_dev,
        })
    }

    /// Upload an RGBA frame from host memory and convert it on GPU. The
    /// result lives in `self.y_dev` / `self.uv_dev` (use [`y_device_ptr`]
    /// / [`uv_device_ptr`] to hand them to ffmpeg).
    pub fn convert(&mut self, rgba_host: &[u8]) -> Result<(), MovieError> {
        let expected = (self.width as usize) * (self.height as usize) * 4;
        if rgba_host.len() != expected {
            return Err(MovieError::CustomError(format!(
                "RGBA buffer is {} bytes, expected {} (= {}x{}x4)",
                rgba_host.len(),
                expected,
                self.width,
                self.height
            )));
        }

        // Upload host RGBA → device RGBA. memcpy_htod_into reuses the
        // pre-allocated buffer rather than re-allocating per frame.
        self.stream
            .memcpy_htod(rgba_host, &mut self.rgba_dev)
            .map_err(map_err)?;

        // Launch one thread per 2x2 RGBA block.
        let blocks_x = ((self.width + 1) / 2 + 15) / 16;
        let blocks_y = ((self.height + 1) / 2 + 15) / 16;
        let cfg = LaunchConfig {
            grid_dim: (blocks_x, blocks_y, 1),
            block_dim: (16, 16, 1),
            shared_mem_bytes: 0,
        };

        let w = self.width as i32;
        let h = self.height as i32;
        let zero = 0i32;

        unsafe {
            self.stream
                .launch_builder(&self.kernel)
                .arg(&self.rgba_dev)
                .arg(&mut self.y_dev)
                .arg(&mut self.uv_dev)
                .arg(&w)
                .arg(&h)
                .arg(&zero) // rgba_pitch (0 = use width*4)
                .arg(&zero) // y_pitch
                .arg(&zero) // uv_pitch
                .launch(cfg)
                .map_err(map_err)?;
        }

        Ok(())
    }

    /// Block until the most recent `convert` call has finished. Required
    /// before reading the device buffers from the host or handing the
    /// device pointers to ffmpeg synchronously. NVENC's CUDA path can
    /// take async device pointers if both share the same stream — that
    /// optimization waits for M3-3.
    pub fn synchronize(&self) -> Result<(), MovieError> {
        self.stream.synchronize().map_err(map_err)
    }

    /// Diagnostic: copy the Y plane back to host memory. Used by the
    /// parity test against CPU sws_scale.
    pub fn read_y_to_host(&self) -> Result<Vec<u8>, MovieError> {
        self.stream.clone_dtoh(&self.y_dev).map_err(map_err)
    }

    /// Diagnostic companion to [`read_y_to_host`].
    pub fn read_uv_to_host(&self) -> Result<Vec<u8>, MovieError> {
        self.stream.clone_dtoh(&self.uv_dev).map_err(map_err)
    }

    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }

    /// Copy our converted Y/UV planes into ffmpeg's pooled CUDA frame
    /// using cuMemcpy2D (handles destination pitch / linesize).
    ///
    /// SAFETY: `dst_y_ptr` and `dst_uv_ptr` must be valid CUDA device
    /// pointers in any context on the same device (UVA makes this OK).
    /// Caller must ensure the destination has at least
    /// `dst_y_pitch * height` bytes for Y and `dst_uv_pitch * height/2`
    /// bytes for UV.
    pub unsafe fn copy_to_device_2d(
        &self,
        dst_y_ptr: u64,
        dst_y_pitch: usize,
        dst_uv_ptr: u64,
        dst_uv_pitch: usize,
    ) -> Result<(), MovieError> {
        use cudarc::driver::sys::{cuMemcpy2DAsync_v2, CUDA_MEMCPY2D_st, CUmemorytype};
        use cudarc::driver::DevicePtr;

        let stream_handle = self.stream.cu_stream();
        let w = self.width as usize;
        let h = self.height as usize;
        let null_host: *const std::ffi::c_void = std::ptr::null();
        let null_host_mut: *mut std::ffi::c_void = std::ptr::null_mut();
        let null_array: cudarc::driver::sys::CUarray = std::ptr::null_mut();

        // --- Y plane ---
        let (y_src_ptr, _record_y) = self.y_dev.device_ptr(&self.stream);
        let yc = CUDA_MEMCPY2D_st {
            srcXInBytes: 0,
            srcY: 0,
            srcMemoryType: CUmemorytype::CU_MEMORYTYPE_DEVICE,
            srcHost: null_host,
            srcDevice: y_src_ptr,
            srcArray: null_array,
            srcPitch: w, // tight
            dstXInBytes: 0,
            dstY: 0,
            dstMemoryType: CUmemorytype::CU_MEMORYTYPE_DEVICE,
            dstHost: null_host_mut,
            dstDevice: dst_y_ptr,
            dstArray: null_array,
            dstPitch: dst_y_pitch,
            WidthInBytes: w,
            Height: h,
        };
        let rc = cuMemcpy2DAsync_v2(&yc as *const _, stream_handle);
        if rc != cudarc::driver::sys::CUresult::CUDA_SUCCESS {
            return Err(MovieError::CustomError(format!(
                "cuMemcpy2DAsync_v2 (Y plane) failed: {:?}", rc
            )));
        }

        // --- UV plane ---
        let (uv_src_ptr, _record_uv) = self.uv_dev.device_ptr(&self.stream);
        let uvc = CUDA_MEMCPY2D_st {
            srcXInBytes: 0,
            srcY: 0,
            srcMemoryType: CUmemorytype::CU_MEMORYTYPE_DEVICE,
            srcHost: null_host,
            srcDevice: uv_src_ptr,
            srcArray: null_array,
            srcPitch: w,
            dstXInBytes: 0,
            dstY: 0,
            dstMemoryType: CUmemorytype::CU_MEMORYTYPE_DEVICE,
            dstHost: null_host_mut,
            dstDevice: dst_uv_ptr,
            dstArray: null_array,
            dstPitch: dst_uv_pitch,
            WidthInBytes: w,
            Height: h / 2,
        };
        let rc = cuMemcpy2DAsync_v2(&uvc as *const _, stream_handle);
        if rc != cudarc::driver::sys::CUresult::CUDA_SUCCESS {
            return Err(MovieError::CustomError(format!(
                "cuMemcpy2DAsync_v2 (UV plane) failed: {:?}", rc
            )));
        }
        Ok(())
    }

    /// Internal accessor for M3-3 (hwframe ctx wraps these device pointers).
    #[allow(dead_code)]
    pub(crate) fn y_slice(&self) -> &CudaSlice<u8> { &self.y_dev }
    #[allow(dead_code)]
    pub(crate) fn uv_slice(&self) -> &CudaSlice<u8> { &self.uv_dev }
    #[allow(dead_code)]
    pub(crate) fn cuda_context(&self) -> &Arc<CudaContext> { &self.ctx }
}

fn map_err<E: std::fmt::Debug>(e: E) -> MovieError {
    MovieError::CustomError(format!("cuda: {e:?}"))
}
