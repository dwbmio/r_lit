use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    error::MovieError,
    stage::{model::meta_scene::MetaSceneList, scene::Scene},
    MoveMakerResult, RuntimeCtx,
};

/// Manages all loaded scenes for a render session.
///
/// Storage uses [`BTreeMap`] (not `HashMap`) so iteration order is
/// deterministic across runs — important when later milestones start
/// hashing/diffing rendered output for reproducibility checks.
pub struct StageMgr {
    pub scenes: BTreeMap<String, Scene>,
    pub scenes_meta: MetaSceneList,

    // 所有用到的纹理  是上下文纹理的id
    pub textures: Vec<u32>,
}

impl StageMgr {
    pub fn new(scenes_meta: MetaSceneList) -> Self {
        Self {
            scenes: BTreeMap::new(),
            scenes_meta,
            textures: vec![],
        }
    }

    /// Preload textures for the scene at `idx` and register it under its own
    /// `name` (previously was hardcoded as `"mvp"`, which silently lost any
    /// scene with a different name and broke multi-scene support).
    pub fn meta_scene_preload(&mut self, rtx: &mut RuntimeCtx, idx: u8) -> MoveMakerResult<()> {
        let meta = self
            .scenes_meta
            .meta_scene_list
            .get(idx as usize)
            .ok_or_else(|| {
                MovieError::CustomError(format!(
                    "meta_scene_preload: index {idx} out of range (have {} scenes)",
                    self.scenes_meta.meta_scene_list.len()
                ))
            })?
            .clone();

        let mut s = Scene::new(&meta.name, &meta);
        s.sync_load_dependencies_textures(rtx)?;
        self.scenes.insert(meta.name, s);
        Ok(())
    }

    /// Render the scene named `scene_name` to `output`. Replaces the previous
    /// implementation that always looked up `"mvp"`.
    pub fn start_gen(
        &mut self,
        rtx: &mut RuntimeCtx,
        output: &PathBuf,
        scene_name: &str,
    ) -> MoveMakerResult<()> {
        if !self.scenes.contains_key(scene_name) {
            return Err(MovieError::CustomError(format!(
                "start_gen: scene '{scene_name}' not registered (have: {:?})",
                self.scenes.keys().collect::<Vec<_>>()
            )));
        }
        // Safe to unwrap: contains_key just verified it.
        let scene = self.scenes.get_mut(scene_name).expect("scene exists");
        crate::ffmpeg_inc::create_scene_stream(rtx, output, scene)?;
        Ok(())
    }

    /// Convenience: render the most recently preloaded scene without naming it.
    /// Most callers preload exactly one scene then render it; this avoids the
    /// scene-name boilerplate at the call site.
    pub fn start_gen_first(
        &mut self,
        rtx: &mut RuntimeCtx,
        output: &PathBuf,
    ) -> MoveMakerResult<()> {
        let name = self
            .scenes
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| MovieError::CustomError("start_gen_first: no scene preloaded".into()))?;
        self.start_gen(rtx, output, &name)
    }
}
