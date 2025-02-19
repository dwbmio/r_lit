use super::meta_node::{NodePos, NodeScale};
#[allow(unused)]
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MetaAction {
    #[serde(skip_deserializing)]
    pub bind_node: u64,
    pub action: String,
    pub start_t: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_target: Option<NodePos>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pos_star: Option<NodePos>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale_target: Option<NodeScale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale_star: Option<NodeScale>,
}
impl MetaAction {
    pub fn new_activate(bind_node: u64, start_t: f32, is_active: bool) -> Self {
        Self {
            bind_node,
            action: "active".to_owned(),
            start_t: start_t,
            duration: Some(10.0),
            pos_target: None,
            pos_star: None,
            active: Some(is_active),
            ..Default::default()
        }
    }

    pub fn new_move_to(
        bind_node: u64,
        pos_star: NodePos,
        pos_target: NodePos,
        start_t: f32,
        duration: f32,
    ) -> Self {
        Self {
            bind_node,
            action: "move_to".to_owned(),
            start_t,
            duration: Some(duration),
            pos_target: Some(pos_target),
            pos_star: Some(pos_star),
            ..Default::default()
        }
    }

    pub fn new_scale_to(
        bind_node: u64,
        scale_star: NodeScale,
        scale_target: NodeScale,
        start_t: f32,
        duration: f32,
    ) -> Self {
        Self {
            bind_node,
            action: "scale_to".to_owned(),
            start_t,
            duration: Some(duration),
            scale_target: Some(scale_target),
            scale_star: Some(scale_star),
            ..Default::default()
        }
    }
    

    pub fn is_in_action(&self, t: f32) -> bool {
        if t >= self.start_t && t < self.start_t + self.duration.unwrap_or(0.0) {
            return true;
        }
        return false;
    }
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Default, Hash, Eq, Clone)]
pub enum NodeAttr {
    #[serde(rename = "show")]
    Show,
    #[serde(rename = "hide")]
    Hide,
    #[serde(rename = "pos")]
    Pos,
    #[default]
    Nothing,
}

#[allow(unused)]
impl NodeAttr {
    /// 将字符串转换为枚举
    fn from_str(action: &str) -> Option<Self> {
        match action {
            "show" => Some(NodeAttr::Show),
            "hide" => Some(NodeAttr::Hide),
            "pos" => Some(NodeAttr::Pos),
            _ => Some(NodeAttr::Nothing),
        }
    }

    /// 将枚举转换为字符串
    fn as_str(&self) -> &'static str {
        match self {
            NodeAttr::Show => "show",
            NodeAttr::Hide => "hide",
            NodeAttr::Pos => "pos",
            NodeAttr::Nothing => "nothing",
        }
    }
}
