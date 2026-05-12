//! `LocalWorker` — in-process renderer holding persistent CUDA context.
//!
//! The whole point of this type is **amortizing the 284 ms cuda.init
//! tax** measured in M3 across many jobs. The worker is constructed
//! once per OS thread, pays init exactly once, then loops on jobs
//! delivered by the queue.
//!
//! Key invariants:
//!   * `CudaConverter` (NVRTC-compiled kernel + persistent device
//!     buffers) lives for the worker's lifetime.
//!   * `CudaHwContext` (ffmpeg AVHWFramesContext + AVHWDeviceContext)
//!     also lives for the worker's lifetime. **The encoder context
//!     itself is recreated per job** because h264_nvenc holds DPB /
//!     B-frame state that cannot be safely reused across two videos.
//!     Recreating it is ~17 ms (measured in trace_cuda) — acceptable
//!     vs the ~284 ms it would take to also rebuild CUDA + hwframes.
//!   * Workers are NOT moved across threads after construction —
//!     CUDA context affinity is per-thread; the actix `SyncArbiter`
//!     pattern (or a single-thread `Arbiter` per worker) is used by
//!     `Supervisor` (M5-3) to enforce this.

use crate::job::{RenderJob, RenderResult};
use crate::worker::{Worker, WorkerError, WorkerKind};
use async_trait::async_trait;
use gamereel_core::cuda_pipeline::CudaConverter;
use gamereel_core::ffmpeg_inc::hwctx::CudaHwContext;
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::stage::model::meta_scene::MetaSceneList;
use gamereel_core::RuntimeCtx;
use std::time::Instant;

pub struct LocalWorker {
    id: String,

    /// Frame size this worker is committed to. A LocalWorker is
    /// resolution-bound because CudaConverter pre-allocates buffers.
    /// If the caller wants a different resolution mid-batch, the
    /// supervisor must spawn a fresh worker (or the worker can be
    /// designed to handle multi-size with a small cache; deferred).
    width: u32,
    height: u32,

    /// Persistent across jobs. Built once at `new()`, dropped when
    /// the worker is shut down.
    converter: CudaConverter,
    hwctx: CudaHwContext,

    init_wall_ms: u64,
    jobs_completed: u64,
}

impl LocalWorker {
    /// Construct a worker bound to `width × height`. This does the
    /// expensive CUDA init (NVRTC compile + hwframes pool); subsequent
    /// `render()` calls reuse all of it.
    pub fn new(id: impl Into<String>, width: u32, height: u32) -> Result<Self, WorkerError> {
        let id = id.into();
        let t = Instant::now();
        let converter = CudaConverter::new(width, height)
            .map_err(|e| WorkerError::Init(format!("CudaConverter::new: {e}")))?;
        let hwctx = CudaHwContext::new(width, height, 4)
            .map_err(|e| WorkerError::Init(format!("CudaHwContext::new: {e}")))?;
        let init_wall_ms = t.elapsed().as_millis() as u64;
        log::info!(
            "LocalWorker '{id}' initialized in {init_wall_ms} ms ({width}x{height})"
        );
        Ok(Self {
            id,
            width,
            height,
            converter,
            hwctx,
            init_wall_ms,
            jobs_completed: 0,
        })
    }

    pub fn init_wall_ms(&self) -> u64 { self.init_wall_ms }
    pub fn jobs_completed(&self) -> u64 { self.jobs_completed }

    /// Render one job. Replicates the inner pipeline of
    /// `gamereel_core::ffmpeg_inc::create_scene_stream_cuda` but without
    /// recreating CudaConverter / CudaHwContext on every call.
    fn render_blocking(&mut self, job: &RenderJob) -> Result<RenderResult, String> {
        use ffmpeg_next as ffmpeg;
        use ffmpeg::codec;
        use ffmpeg_sys_next as ffsys;

        let job_t = Instant::now();

        // Sync read of scene.meta — avoids the nested-runtime problem
        // we'd hit calling the async `stage::import_scene` from inside
        // the worker_loop's own runtime. The work is a single
        // file read + serde_json parse; no real benefit to async here.
        let bytes = std::fs::read(&job.scene_meta_path)
            .map_err(|e| format!("read scene.meta {}: {e}", job.scene_meta_path.display()))?;
        let scene_meta: MetaSceneList = serde_json::from_slice(&bytes)
            .map_err(|e| format!("parse scene.meta: {e}"))?;

        // Resolve the project root the way RuntimeCtx expects.
        // Convention: scene.meta sits at `<root>/tests/<scene>/scene.meta`
        // and asset paths inside read `tests/<scene>/...`, so `source_root`
        // is two parents up. Caller can override via `RenderJob.source_root`.
        let source_root = job.source_root.clone().unwrap_or_else(|| {
            job.scene_meta_path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."))
        });

        let width = job.width.unwrap_or(self.width);
        let height = job.height.unwrap_or(self.height);
        let fps = job.fps.unwrap_or(30);
        let duration = job.duration_s.unwrap_or(10);
        if width != self.width || height != self.height {
            return Err(format!(
                "worker '{}' is bound to {}x{} but job '{}' requested {}x{}",
                self.id, self.width, self.height, job.id, width, height
            ));
        }

        // Per-job RuntimeCtx + scene preload. The expensive *one-shot*
        // resources (CUDA + hwframes) are persistent on `self`.
        let mut rtx = RuntimeCtx::new(width, height, duration, fps);
        rtx.set_source_path(source_root);
        let mut mgr = StageMgr::new(scene_meta);
        mgr.meta_scene_preload(&mut rtx, 0)
            .map_err(|e| format!("preload: {e}"))?;
        let scene = mgr
            .scenes
            .values_mut()
            .next()
            .ok_or_else(|| "no scene preloaded".to_string())?;
        scene.on_init(&rtx);

        // Per-job ffmpeg encoder + container.
        let mut octx = ffmpeg::format::output(&job.output_path)
            .map_err(|e| format!("open output {}: {e}", job.output_path.display()))?;
        let global_header = octx
            .format()
            .flags()
            .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
        let codec_h264 = codec::encoder::find_by_name("h264_nvenc")
            .ok_or_else(|| "h264_nvenc not available".to_string())?;
        let mut ost = octx.add_stream(codec_h264).map_err(|e| format!("add_stream: {e}"))?;
        let mut enc = codec::context::Context::new_with_codec(codec_h264)
            .encoder()
            .video()
            .map_err(|e| format!("video enc: {e}"))?;
        enc.set_width(width);
        enc.set_height(height);
        enc.set_format(ffmpeg::format::Pixel::CUDA);
        enc.set_frame_rate(Some((fps as i32, 1)));
        enc.set_time_base(ffmpeg::Rational(1, fps as i32));
        if global_header {
            enc.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        }
        unsafe {
            let cc_ptr = enc.as_mut_ptr() as *mut ffsys::AVCodecContext;
            (*cc_ptr).hw_frames_ctx = self.hwctx.frames_ref();
        }
        let mut opts = ffmpeg::Dictionary::new();
        for (k, v) in &[
            ("preset", "p4"), ("tune", "hq"), ("rc", "vbr"), ("cq", "23"),
            ("b:v", "8M"), ("maxrate", "12M"), ("bufsize", "16M"),
            ("profile", "high"), ("bf", "3"),
        ] {
            opts.set(k, v);
        }
        let mut cc = enc.open_with(opts).map_err(|e| format!("open enc: {e}"))?;
        ost.set_parameters(&cc);
        ost.set_time_base(ffmpeg::Rational(1, fps as i32));
        octx.write_header().map_err(|e| format!("write_header: {e}"))?;
        let stream_tb = octx
            .stream(0)
            .ok_or_else(|| "stream 0 missing".to_string())?
            .time_base();

        // Render loop, instrumented for render_loop time.
        let render_t = Instant::now();
        let total_frames = (fps as u64) * duration;
        for f in 0..total_frames {
            let img = scene
                .on_render(&mut rtx, f as f32 / fps as f32)
                .map_err(|e| format!("on_render: {e}"))?;
            let rgba = img.to_rgba8();
            self.converter
                .convert(rgba.as_raw())
                .map_err(|e| format!("cuda convert: {e}"))?;
            unsafe {
                let frame_ptr = self
                    .hwctx
                    .allocate_frame()
                    .map_err(|e| format!("hwframe alloc: {e}"))?;
                let dy = (*frame_ptr).data[0] as u64;
                let duv = (*frame_ptr).data[1] as u64;
                let dyp = (*frame_ptr).linesize[0] as usize;
                let dup = (*frame_ptr).linesize[1] as usize;
                self.converter
                    .copy_to_device_2d(dy, dyp, duv, dup)
                    .map_err(|e| format!("cuda copy_to_device_2d: {e}"))?;
                self.converter.synchronize().map_err(|e| format!("sync: {e}"))?;

                (*frame_ptr).pts = f as i64;
                let cc_ptr = cc.as_mut_ptr() as *mut ffsys::AVCodecContext;
                let send_rc = ffsys::avcodec_send_frame(cc_ptr, frame_ptr);
                if send_rc < 0 {
                    let mut fp = frame_ptr;
                    ffsys::av_frame_free(&mut fp);
                    return Err(format!("avcodec_send_frame rc={send_rc}"));
                }
                let mut fp = frame_ptr;
                ffsys::av_frame_free(&mut fp);
            }

            let mut pkt = ffmpeg::Packet::empty();
            while cc.receive_packet(&mut pkt).is_ok() {
                pkt.set_stream(0);
                pkt.rescale_ts(ffmpeg::Rational(1, fps as i32), stream_tb);
                pkt.write_interleaved(&mut octx)
                    .map_err(|e| format!("write_pkt: {e}"))?;
            }
        }
        cc.send_eof().map_err(|e| format!("send_eof: {e}"))?;
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(0);
            pkt.rescale_ts(ffmpeg::Rational(1, fps as i32), stream_tb);
            pkt.write_interleaved(&mut octx)
                .map_err(|e| format!("flush: {e}"))?;
        }
        octx.write_trailer().map_err(|e| format!("trailer: {e}"))?;
        let render_loop = render_t.elapsed();

        let output_bytes = std::fs::metadata(&job.output_path).map(|m| m.len()).unwrap_or(0);
        self.jobs_completed += 1;

        Ok(RenderResult {
            job_id: job.id.clone(),
            worker_id: self.id.clone(),
            ok: true,
            output_bytes,
            wall: job_t.elapsed(),
            render_loop,
            error: None,
            tag: job.tag.clone(),
        })
    }
}

#[async_trait]
impl Worker for LocalWorker {
    fn id(&self) -> &str { &self.id }
    fn kind(&self) -> WorkerKind { WorkerKind::Local }

    async fn render(&mut self, job: RenderJob) -> Result<RenderResult, WorkerError> {
        // The pool runs each LocalWorker on its own dedicated OS thread
        // with a current-thread tokio runtime — blocking here is fine
        // and intended (the whole point is to pin CUDA + ffmpeg state
        // to one thread). No block_in_place needed.
        match self.render_blocking(&job) {
            Ok(r) => Ok(r),
            Err(reason) => Err(WorkerError::Render { job_id: job.id, reason }),
        }
    }
}
