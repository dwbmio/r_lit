use movie_maker::error::MovieError;
use movie_maker::ffmpeg_inc;
use movie_maker::ffmpeg_inc::stage_mgr::StageMgr;
use movie_maker::RuntimeCtx;
use std::path::Path;

mod nodes;
mod report;

fn main() -> Result<(), MovieError> {
    ffmpeg_inc::init_env().unwrap();
    let project_root = env!("CARGO_MANIFEST_DIR");
    let out_mp4 = Path::new(project_root)
        .join("tests/hs-proj/output2.mp4")
        .to_path_buf();

    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    // debug
    let block_img_path = Path::new(project_root).join("tests/hs-proj/48.png");
    println!("local block image : {:?}", block_img_path);
    let c: Result<(), MovieError> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let _ = &rtx.set_source_path(Path::new(project_root).to_path_buf());
            let now = std::time::Instant::now(); // 程序起始时间
            println!("instant: {}", now.elapsed().as_millis());

            // scene meta
            // 用户初始化静态对象
            let path_scene_meta = Path::new(project_root).join("tests/hs-proj/scene.meta");
            let scene_meta = movie_maker::stage::import_scene(path_scene_meta)
                .await
                .expect("load meta failed!");

            // scene dynamic meta
            // 动态根据战报生成timeline
            let report = report::Report::new();
            report.gen_report_dynamic_images(&mut rtx);
            // stage from scene-meta to generate video
            let mut stage = StageMgr::new( scene_meta);
            report.gen_nodes(&mut rtx, &mut stage.scenes_meta.meta_scene_list[0]);
            stage.meta_scene_preload(&mut rtx, 0).expect("preload failed!");
            stage.scenes_meta.meta_scene_list[0].dump_to_file();    //debug 
            stage.start_gen(&mut rtx,&out_mp4)?;

            let end = now.elapsed().as_millis();
            println!("[movie-maker]draw call times: {}",rtx.draw_call_times);
            println!("程序运行了 {:?} ms", end); // 程序终止时间
            Ok(())
        });
    c
}
