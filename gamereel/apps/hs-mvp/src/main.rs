use gamereel_core::error::GamereelError;
use gamereel_core::ffmpeg_inc;
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::RuntimeCtx;
use std::path::Path;

mod nodes;
mod report;

fn main() -> Result<(), GamereelError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    ffmpeg_inc::init_env().unwrap();
    let project_root = env!("CARGO_MANIFEST_DIR");
    let out_mp4 = Path::new(project_root)
        .join("tests/hs-proj/output2.mp4")
        .to_path_buf();

    // D3: pipeline selection via env var. Default = cuda (M3 path) so
    // the demo benefits from the CUDA hwframes pipeline by default.
    // Set GAMEREEL_PIPELINE=sws to fall back to the M2 sws_scale path
    // (useful on hosts without NVIDIA driver + libnvrtc).
    let use_cuda = std::env::var("GAMEREEL_PIPELINE")
        .map(|v| v != "sws")
        .unwrap_or(true);

    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    let c: Result<(), GamereelError> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let _ = &rtx.set_source_path(Path::new(project_root).to_path_buf());
            let now = std::time::Instant::now();

            let path_scene_meta = Path::new(project_root).join("tests/hs-proj/scene.meta");
            let scene_meta = gamereel_core::stage::import_scene(path_scene_meta)
                .await
                .expect("load meta failed!");

            let report = report::Report::new();
            report.gen_report_dynamic_images(&mut rtx);
            let mut stage = StageMgr::new(scene_meta);
            report.gen_nodes(&mut rtx, &mut stage.scenes_meta.meta_scene_list[0]);
            stage.meta_scene_preload(&mut rtx, 0)?;
            // dump_to_file() removed: ~5ms debug overhead, see optimization-log O-017.

            if use_cuda {
                log::info!("hs-mvp: using CUDA pipeline (M3); set GAMEREEL_PIPELINE=sws to fall back");
                stage.start_gen_first_cuda(&mut rtx, &out_mp4)?;
            } else {
                log::info!("hs-mvp: using sws_scale pipeline (M2)");
                stage.start_gen_first(&mut rtx, &out_mp4)?;
            }

            let end = now.elapsed().as_millis();
            println!("[gamereel-core] draw call times: {}", rtx.draw_call_times);
            println!("程序运行了 {} ms", end);
            Ok(())
        });
    c
}
