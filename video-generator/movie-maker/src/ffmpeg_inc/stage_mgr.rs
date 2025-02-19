use std::{collections::HashMap, path::PathBuf};

use crate::{
    stage::{model::meta_scene::MetaSceneList, scene::Scene},
    MoveMakerResult, RuntimeCtx,
};

pub struct StageMgr {
    pub scenes: HashMap<String, Scene>,
    pub scenes_meta: MetaSceneList,

    // 所有用到的纹理  是上下文纹理的id
    pub textures: Vec<u32>,
}

impl StageMgr {
    // region: public
    pub fn new(scenes_meta: MetaSceneList) -> Self {
        Self {
            scenes: HashMap::new(),
            scenes_meta: scenes_meta,
            textures: vec![],
        }
    }

    ///
    /// 根据场景meta加载所有需要的资源
    pub fn meta_scene_preload(&mut self, rtx: &mut RuntimeCtx, idx: u8) -> MoveMakerResult<()> {
        let scene_meta = self.scenes_meta.meta_scene_list.get(idx as usize);
        if let Some(meta) = scene_meta {
            let mut s = Scene::new(&meta.name, &meta);
            // load 资源
            let _ = s.sync_load_dependencies_textures(rtx);
            self.scenes.insert("mvp".to_owned(), s);
        }
        Ok(())
    }


    ///
    /// 开始生成视频
    pub fn start_gen(&mut self, rtx: &mut RuntimeCtx, output: &PathBuf) -> MoveMakerResult<()> {
        crate::ffmpeg_inc::create_scene_stream(
            rtx,
            output, 
            &mut self.scenes.get_mut("mvp").expect("try get scene")
        )?;
        Ok(())
    }
}
