pub mod encoder_pick;
pub mod stage_mgr;
pub mod stream;
pub mod texture;
pub mod image_effect;

use std::path::PathBuf;

use crate::{stage::scene::Scene, MoveMakerResult, RuntimeCtx};
use ffmpeg_next::{self as ffmpeg};
use image::GenericImageView as _;

use encoder_pick::{pick_h264_encoder, EncoderChoice, EncoderPreference};

pub fn init_env() -> crate::MoveMakerResult<()> {
    ffmpeg_next::init()?;
    Ok(())
}

/// Renders a scene to MP4. M1 changes vs the original implementation:
///   * Encoder is auto-selected (NVENC > QSV > VAAPI > VideoToolbox > libx264).
///   * Scaler and intermediate frames are constructed once and reused.
///   * No more `unwrap()` on encoder lookup; failure path returns MovieError.
pub fn create_scene_stream(
    ctx: &mut RuntimeCtx,
    output: &PathBuf,
    scene_inc: &mut Scene,
) -> MoveMakerResult<()> {
    create_scene_stream_with(ctx, output, scene_inc, EncoderPreference::AutoBalanced)
}

/// As [`create_scene_stream`] but lets the caller force a specific encoder
/// preference (used by benches and quality-eval to lock x264 baseline).
pub fn create_scene_stream_with(
    ctx: &mut RuntimeCtx,
    output: &PathBuf,
    scene_inc: &mut Scene,
    pref: EncoderPreference,
) -> MoveMakerResult<()> {
    let width = ctx.view_port.width;
    let height = ctx.view_port.height;
    let fps = ctx.stream.fps as i32;
    let duration = ctx.stream.duration.as_secs(); // seconds
    let total_frames = (fps as u64) * duration;

    // 1) Container.
    let mut octx = ffmpeg::format::output(output)?;
    let global_header = octx
        .format()
        .flags()
        .contains(ffmpeg::format::Flags::GLOBAL_HEADER);

    // 2) Encoder selection + opts.
    let choice: EncoderChoice = pick_h264_encoder(pref)?;
    let codec = ffmpeg::codec::encoder::find_by_name(choice.codec_name).ok_or_else(|| {
        crate::error::MovieError::CustomError(format!(
            "encoder '{}' was selected but find_by_name returned None",
            choice.codec_name
        ))
    })?;
    log::info!(
        "movie-maker: selected encoder '{}' ({})",
        choice.codec_name,
        choice.profile_label
    );

    // 3) Stream + encoder context.
    let mut ost = octx.add_stream(codec)?;
    let mut encoder_ctx = ffmpeg::codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    encoder_ctx.set_width(width);
    encoder_ctx.set_height(height);
    encoder_ctx.set_format(choice.pixel_format);
    encoder_ctx.set_frame_rate(Some((fps, 1)));
    encoder_ctx.set_time_base(ffmpeg::Rational(1, fps));
    encoder_ctx.set_threading(ffmpeg::codec::threading::Config::kind(
        ffmpeg::codec::threading::Type::Slice,
    ));
    if global_header {
        encoder_ctx.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
    }

    // 4) Open with codec-specific options.
    let mut opts = ffmpeg::Dictionary::new();
    for (k, v) in &choice.opts {
        opts.set(k, v);
    }
    let mut cc = encoder_ctx.open_with(opts)?;

    ost.set_parameters(&cc);
    ost.set_time_base(ffmpeg::Rational(1, fps));
    octx.write_header()?;
    let stream_time_base = octx
        .stream(0)
        .ok_or_else(|| {
            crate::error::MovieError::CustomError("output stream 0 missing after write_header".into())
        })?
        .time_base();

    // 5) Scene init: rendering one probe frame so we can size the scaler.
    scene_inc.on_init(ctx);
    let probe = scene_inc.on_render(ctx, 0.0)?;
    let (src_w, src_h) = probe.dimensions();

    let mut scaler = ffmpeg::software::scaling::Context::get(
        ffmpeg::format::Pixel::RGBA,
        src_w,
        src_h,
        choice.pixel_format,
        width,
        height,
        ffmpeg::software::scaling::Flags::BILINEAR,
    )?;
    let mut rgba_frame = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, src_w, src_h);
    let mut yuv_frame = ffmpeg::frame::Video::new(choice.pixel_format, width, height);

    // Helper: copy a DynamicImage's RGBA bytes into the persistent rgba_frame.
    // ffmpeg's RGBA frame uses a single linesize. If the texture's row size in
    // bytes equals the frame's linesize we can copy in one shot; otherwise we
    // copy row-by-row to handle any padding.
    let copy_rgba = |frame: &mut ffmpeg::frame::Video, img: &image::DynamicImage| {
        let rgba_buf = img.to_rgba8();
        let raw = rgba_buf.as_raw();
        let linesize = frame.stride(0);
        let row_bytes = (src_w as usize) * 4;
        let dst = frame.data_mut(0);
        if linesize == row_bytes {
            dst[..raw.len()].copy_from_slice(raw);
        } else {
            for y in 0..src_h as usize {
                let src_off = y * row_bytes;
                let dst_off = y * linesize;
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&raw[src_off..src_off + row_bytes]);
            }
        }
    };

    // Helper: send one fully-prepared yuv frame and drain packets.
    let mut send_and_drain =
        |cc: &mut ffmpeg::encoder::Video,
         octx: &mut ffmpeg::format::context::Output,
         frame_opt: Option<&ffmpeg::frame::Video>|
         -> MoveMakerResult<()> {
            match frame_opt {
                Some(f) => cc.send_frame(f)?,
                None => cc.send_eof()?,
            }
            let mut packet = ffmpeg::Packet::empty();
            while cc.receive_packet(&mut packet).is_ok() {
                packet.set_stream(0);
                packet.rescale_ts(ffmpeg::Rational(1, fps), stream_time_base);
                packet.write_interleaved(octx)?;
            }
            Ok(())
        };

    // 6) Frame 0 (already rendered as probe).
    copy_rgba(&mut rgba_frame, &probe);
    scaler.run(&rgba_frame, &mut yuv_frame)?;
    yuv_frame.set_pts(Some(0));
    send_and_drain(&mut cc, &mut octx, Some(&yuv_frame))?;

    // 7) Remaining frames.
    for frame_number in 1..total_frames {
        let rgba_texture = scene_inc.on_render(ctx, frame_number as f32 / fps as f32)?;
        copy_rgba(&mut rgba_frame, &rgba_texture);
        scaler.run(&rgba_frame, &mut yuv_frame)?;
        yuv_frame.set_pts(Some(frame_number as i64));
        send_and_drain(&mut cc, &mut octx, Some(&yuv_frame))?;
    }

    // 8) Flush.
    send_and_drain(&mut cc, &mut octx, None)?;
    octx.write_trailer()?;
    log::info!(
        "movie-maker: wrote {} ({} frames @ {}x{} via {})",
        output.display(),
        total_frames,
        width,
        height,
        choice.codec_name
    );
    Ok(())
}
