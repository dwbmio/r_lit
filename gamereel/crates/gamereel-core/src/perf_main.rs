use std::path::Path;

use gamereel_core::error::GamereelError;
use gamereel_core::ffmpeg_inc;
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::RuntimeCtx;

fn main() -> Result<(), GamereelError> {
    // Default to INFO so encoder selection is visible without RUST_LOG fiddling.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    ffmpeg_inc::init_env()?;
    let project_root = env!("CARGO_MANIFEST_DIR");
    let path_buf = Path::new(project_root)
        .join("tests/perf_main/icon.png")
        .to_path_buf();
    println!("file path:{:?}", path_buf);

    let out_mp4 = Path::new(project_root)
        .join("tests/perf_main/output2.mp4")
        .to_path_buf();

    let dur_secs: u64 = std::env::var("GAMEREEL_DURATION_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let mut rtx = RuntimeCtx::new(720, 1080, dur_secs, 30);
    let _ = rtx.init(Some(Path::new(project_root).to_path_buf()));
    let c: Result<(), GamereelError> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let now = std::time::Instant::now(); // 程序起始时间
            println!("now{:?}", now);
            println!("async load here!");
            // scene meta
            let path_scene_meta = Path::new(project_root).join("tests/perf_main/scene.meta");
            let scene_meta = gamereel_core::stage::import_scene(path_scene_meta)
                .await
                .expect("load meta failed!");

            let mut stage = StageMgr::new(scene_meta);
            stage.meta_scene_preload(&mut rtx, 0)?;
            stage.start_gen_first(&mut rtx, &out_mp4)?;
            println!("[runtime ctx] draw call times: {}", rtx.draw_call_times);
            let end = now.elapsed().as_millis();
            println!("程序运行了 {:?} millis", end); // 程序终止时间
            Ok(())
        });
    c
}
