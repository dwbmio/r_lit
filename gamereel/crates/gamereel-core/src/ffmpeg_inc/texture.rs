use image::{DynamicImage, GenericImageView};
use std::path::PathBuf;
use std::sync::Arc;

use crate::GamereelResult;

#[allow(unused)]
#[derive(Debug, Default)]
pub struct Texture {
    pub id: String,
    pub name: Option<String>,   //创建一个名字 方便识别动态创建的纹理
    origin_width: u32,
    origin_height: u32,
    graph_width: u32,
    graph_height: u32,
    /// Stored as `Arc<DynamicImage>` so the per-frame compose loop in
    /// `Scene::on_render` can hand out cheap refcount-clones instead of
    /// deep-copying the entire image buffer once per node per frame.
    /// Pre-D2 this was `Option<DynamicImage>`; the `.clone()` calls on
    /// hs-mvp added up to ~15-20% of compose time.
    pub dynamic_image: Option<Arc<DynamicImage>>,
}

#[allow(unused)]
impl Texture {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            ..Default::default()
        }
    }

    pub fn new_with_name(id: &str, name: &str) -> Self {
        Self {
            id: id.to_owned(), 
            name: Some(name.to_owned()), 
            ..Default::default()
        }
    }
}

impl Texture {
    pub fn set_graph_size(&mut self, width: u32, height: u32) {
        self.graph_width = width;
        self.graph_height = height;
    }

    pub fn load_texture(&mut self, tp: &PathBuf) -> GamereelResult<()> {
        let img = image::open(tp)?;
        let (width, height) = &img.dimensions();
        self.origin_width = width.to_owned();
        self.origin_height = height.to_owned();
        self.graph_width = self.origin_width;
        self.graph_height = self.origin_height;
        self.dynamic_image = Some(Arc::new(img));
        Ok(())
    }

    pub fn set_texture(&mut self, tp: &DynamicImage) -> GamereelResult<()> {
        let (width, height) = tp.dimensions();
        self.origin_width = width.to_owned();
        self.origin_height = height.to_owned();
        self.graph_width = self.origin_width;
        self.graph_height = self.origin_height;
        // tp.to_owned() is one buffer copy at upload; from then on the
        // Arc is shared with no further deep copies.
        self.dynamic_image = Some(Arc::new(tp.to_owned()));
        Ok(())
    }

    pub fn load_clear_texture(&mut self) -> GamereelResult<()> {
        let canvas = DynamicImage::new_rgba8(self.graph_width, self.graph_height);
        self.dynamic_image = Some(Arc::new(canvas));
        Ok(())
    }
}
