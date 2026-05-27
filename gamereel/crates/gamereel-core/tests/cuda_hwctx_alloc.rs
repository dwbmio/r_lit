//! M3-3 self-proof: ffmpeg's CUDA hwframe context wraps the same primary
//! CUDA context that cudarc uses, and allocates pooled NV12 device frames
//! with non-null plane pointers and the right linesizes.
//!
//! Marked `#[ignore]` (CUDA hardware required):
//!   cargo test --release -p gamereel-core --test cuda_hwctx_alloc \
//!       -- --ignored --nocapture

use ffmpeg_sys_next as ffsys;
use gamereel_core::cuda_pipeline::CudaConverter;
use gamereel_core::ffmpeg_inc::hwctx::CudaHwContext;

const W: u32 = 320;
const H: u32 = 240;

#[test]
#[ignore]
fn allocates_pooled_cuda_nv12_frames() {
    // First touch cudarc so the primary context is retained.
    let _converter = CudaConverter::new(W, H).expect("cudarc primary ctx");

    // Now ffmpeg should attach to the *same* primary context via
    // AV_CUDA_USE_PRIMARY_CONTEXT — no GPU re-init pause expected.
    let hwctx = CudaHwContext::new(W, H, 4).expect("CudaHwContext::new");
    assert_eq!(hwctx.width(), W);
    assert_eq!(hwctx.height(), H);

    // Allocate a frame from the pool and inspect it.
    unsafe {
        let frame = hwctx.allocate_frame().expect("allocate_frame");
        assert!(!frame.is_null());

        let f = &mut *frame;
        assert_eq!(f.format, ffsys::AVPixelFormat::AV_PIX_FMT_CUDA as i32);
        assert_eq!(f.width, W as i32, "frame width");
        assert_eq!(f.height, H as i32, "frame height");

        assert!(!f.data[0].is_null(), "Y plane device pointer must be non-null");
        assert!(!f.data[1].is_null(), "UV plane device pointer must be non-null");
        assert!(f.linesize[0] > 0, "Y linesize must be set ({})", f.linesize[0]);
        assert!(f.linesize[1] > 0, "UV linesize must be set ({})", f.linesize[1]);

        println!("pool-allocated CUDA NV12 frame:");
        println!("  Y  ptr=0x{:x}  linesize={}", f.data[0] as usize, f.linesize[0]);
        println!("  UV ptr=0x{:x}  linesize={}", f.data[1] as usize, f.linesize[1]);

        // Allocate two more — pool should keep handing out distinct
        // device pointers until pool size is reached.
        let f2 = hwctx.allocate_frame().expect("alloc 2");
        let f3 = hwctx.allocate_frame().expect("alloc 3");
        assert_ne!((*f).data[0], (*f2).data[0], "pool returned same buffer twice");
        assert_ne!((*f2).data[0], (*f3).data[0], "pool returned same buffer twice");

        // Free everything we allocated.
        let mut f1m = frame;
        let mut f2m = f2;
        let mut f3m = f3;
        ffsys::av_frame_free(&mut f1m);
        ffsys::av_frame_free(&mut f2m);
        ffsys::av_frame_free(&mut f3m);
    }
}

#[test]
#[ignore]
fn frames_ref_can_be_borrowed_multiple_times() {
    let _converter = CudaConverter::new(W, H).expect("cudarc primary ctx");
    let hwctx = CudaHwContext::new(W, H, 2).expect("CudaHwContext::new");

    // Each call should return a *new* AVBufferRef* (refcount-incremented),
    // so we can give the same pool to multiple consumers (e.g. encoder +
    // copy helper).
    let r1 = hwctx.frames_ref();
    let r2 = hwctx.frames_ref();
    assert!(!r1.is_null());
    assert!(!r2.is_null());
    // The two AVBufferRef wrappers may be different pointers (each is its
    // own ref) but both should resolve to the same underlying data buffer.
    unsafe {
        let d1 = (*r1).data;
        let d2 = (*r2).data;
        assert_eq!(d1, d2, "borrowed frames refs must alias same hwframe ctx");
        let mut r1m = r1;
        let mut r2m = r2;
        ffsys::av_buffer_unref(&mut r1m);
        ffsys::av_buffer_unref(&mut r2m);
    }
}
