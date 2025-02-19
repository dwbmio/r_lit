use model::meta_scene::MetaSceneList;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;

pub mod action;
pub mod model;
pub mod node;
pub mod scene;

pub async fn import_scene(data_file: PathBuf) -> crate::MoveMakerResult<MetaSceneList> {
    let mut file_cfg = tokio::fs::File::open(data_file).await?;
    // 创建一个缓冲区来存储文件内容
    let mut cc = vec![];
    // 异步读取文件内容到缓冲区
    file_cfg.read_to_end(&mut cc).await?;
    // 将字符串解析为 serde_json::Value
    let scene_meta: MetaSceneList = serde_json::from_slice(&cc)?;
    // json_value.
    Ok(scene_meta)
}
