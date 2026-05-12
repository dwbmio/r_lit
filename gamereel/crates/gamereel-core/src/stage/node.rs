use super::model::meta_node::{MetaNode, NodePos};
use super::model::meta_node::{NodeAnchor, NodeColor, NodeScale, NodeSize};
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

#[allow(unused)]
#[derive(Debug, Default, Clone)]
pub struct NodeGraph {
    pub id: u64,
    pub name: String,
    pub pos: NodePos,
    pub active: bool,
    pub rotation: Option<f32>,
    pub scale: Option<NodeScale>,
    pub size: Option<NodeSize>,
    pub color: Option<NodeColor>,
    pub opacity: Option<u32>,
    pub anchor: Option<NodeAnchor>,

    pub tp_id: Option<String>,
    pub is_static: bool, //是否是完全静态的节点
    pub is_shared: bool, //是否是独立的
}

impl NodeGraph {
    ///
    /// 直接从meta文件中读取创建的NodeGraph信息
    pub fn new_form_meta(id: u64, name: &str, meta_node: MetaNode) -> Self {
        Self {
            id: id,
            name: name.to_owned(),
            pos: meta_node.attr.pos,
            active: meta_node.attr.active,

            rotation: meta_node.attr.rotation,
            scale: meta_node.attr.scale,
            size: meta_node.attr.size,
            color: meta_node.attr.color,
            opacity: meta_node.attr.opacity,
            anchor: meta_node.attr.anchor,
            tp_id: meta_node.tp_id,
            is_static: meta_node.attr.is_static,
            is_shared: meta_node.attr.is_shared,
            ..Default::default()
        }
    }

    ///
    /// 动态创建的NodeGraph
    /// id临时使用时间戳
    pub fn from_tp_id(name: &str) -> Self {
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");
        Self {
            id: since_the_epoch.as_secs(),
            name: name.to_owned(),
            pos: [0.0, 0.0, 0.0],
            tp_id: None,
            ..Default::default()
        }
    }

    ///
    /// 设置已经在runtime上下文中的纹理数据id
    pub fn set_texture_id(&mut self, tp_id: String) {
        // load_loc_image
        self.tp_id = Some(tp_id);
    }

    ///
    /// AT
    pub fn set_pos(&mut self, x: f32, y: f32) {
        self.pos[0] = x;
        self.pos[1] = y;
    }
}
