use std::collections::HashMap;

use super::{action::ActionList, model::meta_scene::MetaScene, node::NodeGraph};
use crate::{ffmpeg_inc::image_effect::blend_images, MoveMakerResult, RuntimeCtx};
use image::DynamicImage;

// 一个同场景的片段算一个scene
#[allow(unused)]
#[derive(Default, Debug)]
pub struct Scene {
    pub name: String,
    pub tp_id: String,
    pub meta_scene: MetaScene,                    //原始数据
    pub children: HashMap<u64, NodeGraph>,        //节点树
    pub action_list: HashMap<String, ActionList>, //timeline

    _clear_image: DynamicImage,         //背景的引用
    _dynamic_beach_image: DynamicImage, //分层在clear_image上的清屏纹理
    _catch_image: DynamicImage,         // is_dirty = false 缓存的image
    _dirty: bool,                       //是否脏了 重新渲染
    _first_frame: bool,                 //是否是首帧
}

impl Scene {
    pub fn new(name: &str, meta_scene: &MetaScene) -> Self {
        let s = Scene {
            name: name.to_owned(),
            meta_scene: meta_scene.to_owned(),
            children: HashMap::new(), // 为每个元素创建 NodeGraph‘
            _dirty: true,             // 开始脏渲染
            _first_frame: true,       // 首帧
            ..Default::default()
        };
        s
    }

    ///
    /// 根据id搜索node节点
    pub fn get_child_by_id(&self, id: u64) -> Option<&NodeGraph> {
        self.children.get(&id)
    }

    ///
    /// 加载场景依赖的纹理
    pub fn sync_load_dependencies_textures(&mut self, rtx: &mut RuntimeCtx) -> MoveMakerResult<()> {
        match &self.meta_scene.clear_tp_id {
            Some(v) => {
                self.tp_id = v.to_owned();
            }
            None => {}
        };

        let mut idx = 0;
        for tp in self.meta_scene.textures.iter_mut() {
            let _ = rtx.load_loc_image(tp, &idx.to_string());
            idx += 1;
        }
        Ok(())
    }

    fn action_bind_node(&mut self) {
        for (key, meta_action) in &mut self.meta_scene.timeline {
            if let Ok(index) = key.parse::<u64>() {
                if self.children.contains_key(&index) {
                    for meta_action in meta_action.iter_mut() {
                        meta_action.bind_node = index;
                    }
                }
            } else {
                println!("Action key is not a number: {}", key); // 解析失败
            }
        }
    }
    fn init_action_list(&mut self) {
        self.action_list = self
            .meta_scene
            .timeline
            .clone()
            .into_iter()
            .map(|(key, meta_actions)| (key, ActionList::new(meta_actions)))
            .collect();
    }
    fn init_children(&mut self) {
        self.meta_scene.nodes.iter_mut().for_each(|v| {
            self.children
                .insert(v.id, NodeGraph::new_form_meta(v.id, &v.name, v.clone()));
        });
    }

    pub fn on_init(&mut self, ctx: &RuntimeCtx) {
        let clear_image = ctx
            .get_texture(&self.tp_id)
            .dynamic_image
            .clone()
            .expect("Ensure");
        // 初始化 child
        self.init_children();
        // 根据 child 绑定 action
        self.action_bind_node();
        // 需要先修复 配置绑定 在初始化 action
        self.init_action_list();
        // TODO 根据 是否在action 初始化底板
        self._clear_image = clear_image;
    }

    fn blend_image(base:&mut DynamicImage, overlay_img:&DynamicImage, v:&NodeGraph){
        let mut width:Option<f32> = None;
        let mut height:Option<f32> = None;
        if let Some(size) = v.size {
            width = Some(size[0]);
            height = Some(size[1]);
        }
        blend_images(
            base,
            &overlay_img,
            v.pos[0].floor(),
            v.pos[1].floor(),
            width,
            height,
            Some(v.scale.unwrap_or([1.0,1.0])[0]),
            Some(v.scale.unwrap_or([1.0,1.0])[1]),
            v.rotation,
            v.opacity,
            Some(v.anchor.unwrap_or([0.0,0.0])[0]),
            Some(v.anchor.unwrap_or([0.0,0.0])[1])
        );
    }
    pub fn on_render(
        &mut self,
        ctx: &mut RuntimeCtx,
        g_time: f32,
    ) -> MoveMakerResult<DynamicImage> {
        // clear all! rerender
        let is_dirty = self.do_action(g_time)?;
        // not dirty render director by dynamic beach image
        if is_dirty == false && !self._first_frame {
            return Ok(self._catch_image.clone());
        }
        // first frame dynamic_bench_image
        if self._first_frame {
            // 先生成 静态缓存图
            self._dynamic_beach_image = self._clear_image.clone();
            for (_, v) in &self.children {
                if v.is_static {
                    let node_tp = ctx
                        .get_texture(v.tp_id.clone().unwrap_or("".to_owned()).as_str())
                        .dynamic_image
                        .clone()
                        .expect("Ensure");
                    ctx.draw_call_times += 1;

                    Self::blend_image(&mut self._dynamic_beach_image, &node_tp, v);
                }
            }
            // 在静态缓存图的基础上 初始化
            self._catch_image = self._dynamic_beach_image.clone();
            for (_, v) in &self.children {
                if v.active && !v.is_static {
                    let node_tp = ctx
                        .get_texture(v.tp_id.clone().unwrap_or("".to_owned()).as_str())
                        .dynamic_image
                        .clone()
                        .expect("Ensure");
                    ctx.draw_call_times += 1;
                    Self::blend_image(&mut self._catch_image, &node_tp, v);
                }
            }

            self._first_frame = false;
        } else {
            let mut active_frame = self._dynamic_beach_image.clone();
            for (_, v) in &self.children {
                if v.active && !v.is_static {
                    // don't update static
                    let node_tp: DynamicImage = ctx
                        .get_texture(v.tp_id.clone().unwrap_or("".to_owned()).as_str())
                        .dynamic_image
                        .clone()
                        .expect("Ensure");
                    ctx.draw_call_times += 1;

                    Self::blend_image(&mut active_frame, &node_tp, v);
                    
                }
            }
            self._catch_image = active_frame.to_owned();
            return Ok(active_frame);
        }
        Ok(self._catch_image.clone())
    }

    pub fn do_action(&mut self, g_time: f32) -> MoveMakerResult<bool> {
        let mut is_dirty = false;
        for (_, action_list) in &self.action_list {
            // 如果找到匹配的节点，执行 action_list 的 do_actions
            let ret = action_list.iter_do_action(&self, g_time);
            for c in ret.keys() {
                if let Some(ret) = ret.get(c) {
                    // println!("node = {}, ret = {:?}", c, ret);
                    if let Some(child) = self.children.get_mut(c) {
                        if let Some(x) = ret.x{
                            if child.pos[0] != x {
                                is_dirty = true;
                                child.pos[0] = x;
                            }
                        }
                        if let Some(y) = ret.y{
                            if child.pos[1] != y {
                                is_dirty = true;
                                child.pos[1] = y;
                            }
                        }
                        if let Some(active) = ret.active{
                            if child.active != active {
                                is_dirty = true;
                                child.active = active;
                            }
                        }
                        if let Some(rotation) = ret.rotation{
                            if child.rotation != ret.rotation {
                                is_dirty = true;
                                child.rotation = Some(rotation);
                            }
                        }
                        if let Some(scale) = ret.scale{
                            if child.scale != ret.scale {
                                is_dirty = true;
                                child.scale = Some(scale);
                            }
                        }
                        if let Some(size) = ret.size{
                            if child.size != ret.size {
                                is_dirty = true;
                                child.size = Some(size);
                            }
                        }
                        if let Some(color) = ret.color{
                            if child.color != ret.color {
                                is_dirty = true;
                                child.color = Some(color);
                            }
                        }
                        if let Some(opacity) = ret.opacity{
                            if child.opacity != ret.opacity {
                                is_dirty = true;
                                child.opacity = Some(opacity); 
                            }
                        }
                        if let Some(anchor) = ret.anchor{
                            if child.anchor != ret.anchor {
                                is_dirty = true;
                                child.anchor = Some(anchor);
                            }
                        }
                    }
                    
                }
            }
        }

        Ok(is_dirty)
    }
}
