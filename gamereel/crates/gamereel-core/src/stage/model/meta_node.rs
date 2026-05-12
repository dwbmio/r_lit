
use serde::{Deserialize, Serialize};


pub type NodePos = [f32; 3];
pub type NodeScale = [f32; 2];
pub type NodeSize = [f32; 2];
pub type NodeColor = [u32; 4];
pub type NodeAnchor = [f32; 2];

#[derive(PartialEq, Debug, Serialize, Deserialize, Default, Hash, Eq, Clone)]
pub enum NodeAction {
    MoveTo,
    #[default]
    Wait,
}
#[derive(PartialEq, Debug, Serialize, Deserialize, Default, Hash, Eq, Clone)]
pub struct NodeTimeKeyFrame {
    action: NodeAction,
    value: Vec<u32>,
    t: u64
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Default,  Clone)]
pub struct NodeAttr {
    pub pos: NodePos,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<NodeScale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<NodeSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<NodeColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<NodeAnchor>,

    pub active: bool,   //是否是激活的

    #[serde(default)]
    #[serde(rename = "is-static")]
    pub is_static: bool,        //是否是静态节点

    #[serde(default)]
    #[serde(rename="is-shared")]
    pub is_shared: bool ,      //是否是共享的
} 

///
/// 节点的原始数据
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct MetaNode {
    pub id: u64,
    pub tp_id: Option<String>,
    pub name:String,
    // attr
    pub attr: NodeAttr,
}

impl MetaNode {
    pub fn new(id: u64, tp_id: &str, name: &str) -> Self {
        Self {
            id: id,
            name: name.to_owned(),
            tp_id: Some(tp_id.to_owned()),
            ..Default::default()
        }
    }

    pub fn new_shared(id: u64, tp_id: &str, name: &str) -> Self {
        let mut out = Self {
            id: id,
            name: name.to_owned(),
            tp_id: Some(tp_id.to_owned()),
            ..Default::default()
        };
        out.attr.is_shared = true;
        out
    }

    pub fn new_with_attr(id: u64, tp_id: &str, name: &str, attr: NodeAttr) -> Self {
        Self {
            id: id,
            name: name.to_owned(),
            tp_id: Some(tp_id.to_owned()),
            attr: attr
        }
    }
        
}
