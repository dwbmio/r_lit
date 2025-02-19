pub mod frame;
pub mod stage_mgr;
pub mod stream;
pub mod texture;
pub mod image_effect;

use std::path::PathBuf;

use crate::{stage::scene::Scene, MoveMakerResult, RuntimeCtx};
use ffmpeg::codec;
use ffmpeg_next::{self as ffmpeg};
use image::GenericImageView as _;

pub fn init_env() -> crate::MoveMakerResult<()> {
    let _ = ffmpeg_next::init()?;
    Ok(())
}

pub fn create_scene_stream(
    ctx: &mut RuntimeCtx,
    output: &PathBuf,
    scene_inc: &mut Scene,
) -> MoveMakerResult<()> {
    let width = ctx.view_port.width;
    let height = ctx.view_port.height;
    let fps = ctx.stream.fps as u64;
    let duration = ctx.stream.duration.as_secs(); // seconds
    let total_frames = fps * duration;

    // 视频输出的CTX
    let mut octx = ffmpeg::format::output(output)?;
    let global_header = octx //常规的视频流
        .format()
        .flags()
        .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
    let codec: ffmpeg_next::Codec =
        ffmpeg::codec::encoder::find_by_name("h264_videotoolbox").unwrap();

    // 视频输出的流对象
    let mut ost = octx.add_stream(codec)?;

    // 视频encoder
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    encoder.set_width(width);
    encoder.set_threading(ffmpeg::codec::threading::Config::kind(
        ffmpeg::codec::threading::Type::Slice,
    ));
    encoder.set_height(height);
    encoder.set_frame_rate((fps as i32, 1).into());
    encoder.set_format(ffmpeg::format::Pixel::YUV420P);
    encoder.set_time_base(ffmpeg::Rational(1, fps as i32));
    if global_header {
        encoder.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
    }
    let mut cc = encoder.open_as(codec)?; //encoder-> codec
    ost.set_parameters(&cc); //流对象用encoder
    ost.set_time_base(ffmpeg::Rational(1, fps as i32));
    octx.write_header()?;

    scene_inc.on_init(ctx);
    // 渲染帧
    for frame_number in 0..total_frames {
        let rgba_texture = scene_inc.on_render(ctx, frame_number as f32 / fps as f32)?;
        let (_w, _h) = rgba_texture.dimensions();
        let mut scaler = ffmpeg_next::software::scaling::Context::get(
            ffmpeg::format::Pixel::RGBA, // Input format
            _w,
            _h,
            ffmpeg::format::Pixel::YUV420P, // Output format
            width,
            height,
            ffmpeg_next::software::scaling::Flags::BILINEAR,
        )?;

        let mut rgba_frame = ffmpeg_next::frame::Video::new(ffmpeg::format::Pixel::RGBA, _w, _h);
        rgba_frame
            .data_mut(0)
            .copy_from_slice(&rgba_texture.to_rgba8().into_raw());

        let mut yuv_frame =
            ffmpeg_next::frame::Video::new(ffmpeg::format::Pixel::YUV420P, width, height);
        scaler.run(&rgba_frame, &mut yuv_frame)?;

        // Set PTS for the frame
        let time_base = octx
            .stream(0)
            .expect("Stream at index 0 not found")
            .time_base();
        yuv_frame.set_pts(Some(
            (frame_number as i64) * time_base.denominator() as i64
                / cc.frame_rate().numerator() as i64,
        ));
        // println!(
        //     "set frame frame_number: {:?}, pts:{:?}",
        //     frame_number,
        //     yuv_frame.pts()
        // );
        // Send frame to the encoder
        cc.send_frame(&yuv_frame)?;
        // Retrieve and write all packets for this frame
        let mut packet = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut packet).is_ok() {
            packet.write(&mut octx)?;
        }
    }

    // 写入文件尾
    cc.send_eof()?; // 发送结束帧
    let mut packet = ffmpeg::Packet::empty();
    while cc.receive_packet(&mut packet).is_ok() {
        packet.write_interleaved(&mut octx)?;
    }

    // 写入文件尾
    octx.write_trailer()?;
    println!("Video generated at: {}", output.display());

    Ok(())
}
