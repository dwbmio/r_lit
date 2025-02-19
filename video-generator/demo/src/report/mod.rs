use movie_maker::{
    stage::model::{
        meta_action::MetaAction,
        meta_node::{MetaNode, NodeAttr},
        meta_scene::MetaScene,
    },
    RuntimeCtx,
};
use std::collections::HashMap;

use crate::nodes::{
    bans::gen_bans_image, blocks::gen_block_image, BLOCK_BAN_0, BLOCK_BAN_1, BLOCK_BAN_2, BLOCK_CELL_0, BLOCK_CELL_1, BLOCK_CELL_2
};
mod gen_tp;
///
/// 报告类型
pub enum ReportType {
    Keep5Combe,
}

pub struct ReportRuntime {
    bands_nodes: HashMap<u64, u64>,
    picks_nodes: HashMap<u64, u64>,
}

///picks 位置
const PICKS_POS: [[f32; 3]; 3] = [
    [360.0, 900.0, 0.0],
    [360.0, 900.0, 0.0],
    [570.0, 900.0, 0.0],
];

///picks anchor
const PICK_ANCHOR: [f32; 2] = [0.5, 0.5];

///picks size
const PICK_SIZE: [f32; 2] = [150.0, 150.0];

///
/// 报告数据
///
pub struct Report {
    r_type: ReportType,
    bands: HashMap<u64, [[u32; 8]; 8]>,
    picks: HashMap<u64, [[[u32; 3]; 3]; 3]>,

    runtime: Option<ReportRuntime>, //运行时的对象，在初始化后往里面塞对象及绑定关系
}

fn demo_report() -> Report {
    let mut demo_0_bans: HashMap<u64, [[u32; 8]; 8]> = HashMap::new();
    let mut demo_0_picks: HashMap<u64, [[[u32; 3]; 3]; 3]> = HashMap::new();
    let demo_0_actions: HashMap<String, Vec<MetaAction>> = HashMap::new();

    demo_0_bans.insert(50, BLOCK_BAN_0);
    demo_0_bans.insert(150, BLOCK_BAN_1);
    demo_0_bans.insert(250, BLOCK_BAN_2);
    demo_0_picks.insert(0, [BLOCK_CELL_0, BLOCK_CELL_1, BLOCK_CELL_2]);

    let report = Report {
        r_type: ReportType::Keep5Combe,
        bands: demo_0_bans,
        picks: demo_0_picks,
        runtime: None,
    };
    report
}

impl Report {
    pub fn new() -> Report {
        demo_report()
    }

    pub fn gen_report_dynamic_images(&self, rtx: &mut RuntimeCtx) {
        // todo 格子纹理先写死
        let project_root: &str = env!("CARGO_MANIFEST_DIR");
        let block_img_path = std::path::Path::new(project_root).join("tests/hs-proj/48_ex.png");
        let tp_id = rtx
            .load_loc_image(block_img_path.display().to_string().as_str(), "999")
            .unwrap();
        for (k, v) in &self.bands {
            let bans_img = gen_bans_image(
                &rtx.get_texture(&tp_id)
                    .dynamic_image
                    .clone()
                    .expect("required!"),
                *v,
            );
            if let Some(img) = bans_img {
                rtx.set_textures_cache(&img, format!("bans-{}", k).as_str())
                    .expect("set texture failed!1");
            }
        }
        for (k, v) in &self.picks {
            for i in 0..3 {
                let picks_img = gen_block_image(
                    &rtx.get_texture(&tp_id)
                        .dynamic_image
                        .clone()
                        .expect("required!"),
                    v[i],
                );
                if let Some(img) = picks_img {
                    rtx.set_textures_cache(&img, format!("picks-{}-{}", k, i).as_str())
                        .expect("set texture failed!1");
                }
            }
        }
    }

    pub fn gen_nodes(&self, rtx: &mut RuntimeCtx, meta: &mut MetaScene) -> Vec<MetaNode> {
        let mut meta_node_vec: Vec<MetaNode> = meta.nodes.clone();
        let mut meta_timeline: HashMap<String, Vec<MetaAction>> = meta.timeline.clone();

        // bands 绑定 动态图片 添加动画
        for (i, _) in self.bands.clone() {
            // 10000开头的是band
            let node_id = 10000 + i;
            let ban_name = format!("bans-{}", i);
            // node
            let meta_node = MetaNode::new_with_attr(
                node_id,
                &rtx.get_texture_by_name(ban_name.as_str())
                    .expect(
                        format!(
                            "must load texture before gen_nodes!=> want name: {}",
                            ban_name
                        )
                        .as_str(),
                    )
                    .id,
                &ban_name,
                NodeAttr {
                    pos: [360.0, 463.0, 1.0],
                    anchor: Some([0.5, 0.5]),
                    active: false,
                    ..Default::default()
                },
            );
            meta_node_vec.push(meta_node);
            // node-actions
            let meta_action_list = self.gen_bands_timeline(node_id, i as u32);
            meta_timeline.insert(node_id.to_string(), meta_action_list);
        }

        // pick 绑定动态图片
        let k = 0;
        for i in 0..3 {
            // 20000开头的是band
            let node_id = 20000 + i;
            let ban_name: String = format!("picks-{}-{}", k, i);
            // node
            let meta_node = MetaNode::new_with_attr(
                node_id,
                &rtx.get_texture_by_name(ban_name.as_str())
                    .expect(
                        format!(
                            "must load texture before gen_nodes!=> want name: {}",
                            ban_name
                        )
                        .as_str(),
                    )
                    .id,
                &ban_name,
                NodeAttr {
                    pos: PICKS_POS.get(i as usize).unwrap().to_owned(),
                    anchor: Some(PICK_ANCHOR),
                    size: Some(PICK_SIZE),
                    active: true,
                    ..Default::default()
                },
            );
            meta_node_vec.push(meta_node);
            // node-actions
            let meta_action_list = self.gen_picks_timeline(node_id, i as u32);
            meta_timeline.insert(node_id.to_string(), meta_action_list);
        }

        meta.nodes = meta_node_vec.clone();
        meta.timeline = meta_timeline.clone();
        return meta_node_vec;
    }

    pub fn gen_bands_timeline(&self, node_id: u64, idx: u32) -> Vec<MetaAction> {
        println!("gen bands timeline: idx = {}, ret = {}",idx, idx as f32 * 0.5 + 0.5 );
        let mut show_out_action_vec = vec![MetaAction::new_activate(node_id, idx as f32 / 100.0, true)];
        if idx < 2 {
           show_out_action_vec.push(MetaAction::new_activate(node_id, idx as f32 / 100.0 + 1.0, false));
        }
        show_out_action_vec
    }

    pub fn gen_picks_timeline(&self, node_id: u64, idx: u32) -> Vec<MetaAction> {
        let out: Vec<MetaAction> = match idx {
            0 => {
                return vec![
                    MetaAction::new_move_to(
                        node_id,
                        [150.0, 900.0, 0.0],
                        [180.0, 282.0, 1.0],
                        0.1,
                        0.4,
                    ),
                    MetaAction::new_scale_to(
                        node_id,
                        [1.0, 1.0],
                        [216.0 / 150.0, 216.0 / 150.0],
                        0.1,
                        0.4,
                    ),
                    MetaAction::new_activate(node_id, 0.5, false),
                ];
            }
            1 => {
                return vec![
                    MetaAction::new_move_to(
                        node_id,
                        [360.0, 900.0, 0.0],
                        [545.0, 282.0, 1.0],
                        1.1,
                        0.4,
                    ),
                    MetaAction::new_scale_to(
                        node_id,
                        [1.0, 1.0],
                        [216.0 / 150.0, 216.0 / 150.0],
                        1.1,
                        0.4,
                    ),
                    MetaAction::new_activate(node_id, 1.5, false),
                ];
            }
            2 => {
                return vec![
                    MetaAction::new_move_to(
                        node_id,
                        [570.0, 900.0, 0.0],
                        [396.0, 282.0, 1.0],
                        2.1,
                        0.4,
                    ),
                    MetaAction::new_scale_to(
                        node_id,
                        [1.0, 1.0],
                        [216.0 / 150.0, 216.0 / 150.0],
                        2.1,
                        0.4,
                    ),
                    MetaAction::new_activate(node_id, 2.5, false),
                ];
            }
            _ => vec![],
        };
        return out;
    }
}
