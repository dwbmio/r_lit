use std::collections::HashMap;

use tween::TweenValue;

use super::{
    model::{
        meta_action::MetaAction,
        meta_node::{NodeAnchor, NodeColor, NodeScale, NodeSize},
    },
    node::NodeGraph,
    scene::Scene,
};

use std::ops::{Add, Sub};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point(f32, f32);

impl Point {
    /// Moves us towards the other Point by a factor of `t`
    fn lerp(self, other: Self, t: f32) -> Self {
        self.scale(1.0 - t) + other.scale(t)
    }
}

impl Add for Point {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0, self.1 + rhs.1)
    }
}
impl Sub for Point {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0, self.1 - rhs.1)
    }
}
impl TweenValue for Point {
    fn scale(self, scale: f32) -> Self {
        Self(self.0 * scale, self.1 * scale)
    }
}

#[derive(Debug, Default)]
pub struct ActionResult {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub active: Option<bool>,
    pub rotation: Option<f32>,
    pub scale: Option<NodeScale>,
    pub size: Option<NodeSize>,
    pub color: Option<NodeColor>,
    pub opacity: Option<u32>,
    pub anchor: Option<NodeAnchor>,
    pub time_remaining: f32,
    pub is_finished: bool,
}

impl ActionResult {
    pub fn merge(&mut self, other: ActionResult) {
        if let Some(x) = other.x {
            self.x = Some(x);
        }
        if let Some(y) = other.y {
            self.y = Some(y);
        }
        if let Some(active) = other.active {
            self.active = Some(active);
        }
        if let Some(rotation) = other.rotation {
            self.rotation = Some(rotation);
        }
        if let Some(scale) = other.scale {
            self.scale = Some(scale);
        }
        if let Some(size) = other.size {
            self.size = Some(size);
        }
        if let Some(color) = other.color {
            self.color = Some(color);
        }
        if let Some(opacity) = other.opacity {
            self.opacity = Some(opacity);
        }
        if let Some(anchor) = other.anchor {
            self.anchor = Some(anchor);
        }
    }
}

#[allow(unused)]
#[derive(Debug, Default, Clone)]
pub struct Action {
    pub meta_action: MetaAction,
}

impl Action {
    pub fn new(meta_node: MetaAction) -> Self {
        Self {
            meta_action: meta_node,
        }
    }
    pub fn do_action(&self, node: &NodeGraph, act_r_time: f32) -> ActionResult {
        match self.meta_action.action.as_str() {
            "move_to" => {
                // tween
                let origin = self.meta_action.pos_star.unwrap();
                let origin_p = Point(origin[0], origin[1]);

                let target = self.meta_action.pos_target.unwrap();
                let target_p = Point(target[0], target[1]);
                return self.do_move_to(origin_p, target_p, act_r_time);
            }
            "active" => {
                return ActionResult {
                    active: self.meta_action.active,
                    is_finished: act_r_time >= self.meta_action.start_t  && act_r_time < (self.meta_action.start_t + self.meta_action.duration.unwrap_or(0.0)),
                    ..Default::default()
                };
            }
            "scale_to" => {
                // tween

                let origin = self.meta_action.scale_star.unwrap();
                let origin_p = Point(origin[0], origin[1]);

                let target = self.meta_action.scale_target.unwrap();
                let target_p = Point(target[0], target[1]);

                return self.do_scale_to(origin_p, target_p, act_r_time);
            } 
            // "move_to" => {
            //     // tween
            //     let origin = self.meta_action.pos_star.unwrap();
            //     let origin_p = Point(origin[0], origin[1]);

            //     let target = self.meta_action.pos_target.unwrap();
            //     let target_p = Point(target[0], target[1]);
            //     return self.do_move_to(origin_p, target_p, act_r_time);
            // }

            // "move_by" => {
            //     // tween

            //     let target = self.meta_action.pos_target.unwrap();
            //     let target_p = Point(target[0], target[1]);
            //     return self.do_move_to();
            // }
            _ => ActionResult::default(), // `_` 是通配符，用来匹配所有其他值
        }
    }
    pub fn do_move_to(&self, start_pos: Point, end_pos: Point, act_r_time: f32) -> ActionResult {
        let mut out =
            tween::Tweener::linear(start_pos, end_pos, self.meta_action.duration.unwrap_or(0.0));
        let p: Point = out.move_to(act_r_time);
        // 目标坐标
        return ActionResult {
            x: Some(p.0),
            y: Some(p.1),
            time_remaining: act_r_time,
            is_finished: out.is_finished(),
            ..Default::default()
        };
    }
    pub fn do_scale_to(&self, start_pos: Point, end_pos: Point, act_r_time: f32) -> ActionResult {
        let mut out = tween::Tweener::linear(start_pos, end_pos, self.meta_action.duration.unwrap_or(0.0));
        let p: Point = out.move_to(act_r_time);
        // 目标坐标
        return ActionResult {
            scale:Some([p.0, p.1]),
            time_remaining: act_r_time,
            is_finished: out.is_finished(),
            ..Default::default()
        };
    }
}

#[allow(unused)]
#[derive(Debug, Default, Clone)]
pub struct ActionList {
    pub list_meta_action: Vec<MetaAction>,
    pub idx: usize, // 当前处于 action 的idx
}

impl ActionList {
    pub fn new(list_meta_action: Vec<MetaAction>) -> Self {
        Self {
            list_meta_action: list_meta_action.clone(), // 如果 Self 需要所有权，保留克隆
            idx: 0,
        }
    }

    pub fn iter_do_action(&self, scene: &Scene, g_time: f32) -> HashMap<u64, ActionResult> {
        let mut iter_chg_map: HashMap<u64, ActionResult> = HashMap::new();
        for meta in &self.list_meta_action {
            if g_time > meta.start_t {
                let action = Action::new(meta.clone());
                let node = scene.get_child_by_id(meta.bind_node);
                match node {
                    Some(node) => {
                        let act_r_time = g_time - meta.start_t;
                        let ret: ActionResult = action.do_action(node, act_r_time);
                        match iter_chg_map.get_mut(&meta.bind_node) {
                            Some(act_ret) => {
                                // todo!("act_ret 结果质检的叠加")
                                act_ret.merge(ret);
                            }
                            None => {
                                iter_chg_map.insert(meta.bind_node, ret);
                            }
                        };
                    }
                    None => {
                        log::warn!("not find node {}", meta.bind_node);
                    }
                }
            }
        }
        iter_chg_map
    }
}
