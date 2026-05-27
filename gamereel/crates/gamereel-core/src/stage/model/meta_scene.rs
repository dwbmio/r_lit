use super::{meta_action::MetaAction, meta_node::MetaNode};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs::File, io::Write};

///
/// 场景的原始数据
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct MetaScene {
    pub name: String,
    //背景'
    #[serde(alias = "clear-tp-id")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_tp_id: Option<String>,
    //元素浮动在背景上
    #[serde(alias = "node-textures")]
    pub textures: Vec<String>,
    pub nodes: Vec<MetaNode>,
    pub timeline: HashMap<String, Vec<MetaAction>>,
}

impl MetaScene {
    // #[cfg(debug_assertions)]
    pub fn dump_to_file(&self) {
        let out = serde_json::json!(self);
        let mut dump_file = File::create("_scene_debug_dump.meta").unwrap();
        // 将 JSON 数据写入文件
        dump_file
            .write_all(serde_json::to_string_pretty(&out).unwrap().as_bytes())
            .unwrap();
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct MetaSceneList {
    pub meta_scene_list: Vec<MetaScene>,
}
