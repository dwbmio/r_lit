use std::path::Path;

use movie_maker::error::MovieError;
use movie_maker::ffmpeg_inc;
use movie_maker::ffmpeg_inc::stage_mgr::StageMgr;
use movie_maker::RuntimeCtx;

fn main() -> Result<(), MovieError> {
    ffmpeg_inc::init_env().unwrap();
    let project_root = env!("CARGO_MANIFEST_DIR");
    let path_buf = Path::new(project_root)
        .join("tests/perf_main/icon.png")
        .to_path_buf();
    println!("file path:{:?}", path_buf);

    let out_mp4 = Path::new(project_root)
        .join("tests/perf_main/output2.mp4")
        .to_path_buf();

    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    let _ = rtx.init(Some(Path::new(project_root).to_path_buf()));
    let c: Result<(), MovieError> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let now = std::time::Instant::now(); // 程序起始时间
            println!("now{:?}", now);
            println!("async load here!");
            // scene meta
            let path_scene_meta = Path::new(project_root).join("tests/perf_main/scene.meta");
            let scene_meta = movie_maker::stage::import_scene(path_scene_meta)
                .await
                .expect("load meta failed!");

            let mut stage = StageMgr::new(scene_meta);
            stage.meta_scene_preload(&mut rtx, 0).expect("preload failed!");
            stage.start_gen(&mut rtx, &out_mp4)?;
            println!("[runtime ctx] draw call times: {}", rtx.draw_call_times);
            let end = now.elapsed().as_millis();
            println!("程序运行了 {:?} millis", end); // 程序终止时间
            Ok(())
        });
    c
}
