
pub fn test_mvp() -> Result<(), crate::error::MovieError> {
    ffmpeg_inc::init_env()?;
    let project_root = env!("CARGO_MANIFEST_DIR");
    let path_buf = Path::new(project_root)
        .join("tests/statics/icon.png")
        .to_path_buf();
    println!("file path:{:?}", path_buf);

    let out_mp4 = Path::new(project_root)
        .join("tests/statics/output2.mp4")
        .to_path_buf();

    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    let _ = rtx.init(Some(Path::new(project_root).to_path_buf()));
    let out: Result<(), MovieError> = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let now = Instant::now(); // 程序起始时间
            println!("now {:?}", now);
            println!("async load here!");
            // scene meta
            let path_scene_meta = Path::new(project_root).join("tests/hs-proj/scene.meta");
            let scene_meta = movie_maker::stage::import_scene(path_scene_meta)
                .await
                .expect("load meta failed!");
            let mut stage = StageMgr::new(rtx, scene_meta);

            stage.preload(0).expect("preload failed!");
            stage.start_gen(&out_mp4)?;
            let end = now.elapsed().as_secs();
            println!("程序运行了 {:?} 秒", end); // 程序终止时间

            Ok(())
        });
    out
}
