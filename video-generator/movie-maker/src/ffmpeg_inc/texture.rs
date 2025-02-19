use image::{DynamicImage, GenericImageView};
use std::path::PathBuf;

use crate::MoveMakerResult;

#[allow(unused)]
#[derive(Debug, Default)]
pub struct Texture {
    pub id: String,
    pub name: Option<String>,   //创建一个名字 方便识别动态创建的纹理
    origin_width: u32,
    origin_height: u32,
    graph_width: u32,
    graph_height: u32,
    pub dynamic_image: Option<DynamicImage>,
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

    pub fn load_texture(&mut self, tp: &PathBuf) -> MoveMakerResult<()> {
        let img = image::open(tp)?;
        // 获取图片的宽度和高度
        let (width, height) = &img.dimensions();
        self.origin_width = width.to_owned();
        self.origin_height = height.to_owned();
        // 默认使用原始大小
        self.graph_width = self.origin_width;
        self.graph_height = self.origin_height;
        self.dynamic_image = Some(img);
        Ok(())
    }

    pub fn set_texture(&mut self, tp: &DynamicImage) -> MoveMakerResult<()> {
        let (width, height) = tp.dimensions();
        self.origin_width = width.to_owned();
        self.origin_height = height.to_owned();
        // 默认使用原始大小
        self.graph_width = self.origin_width;
        self.graph_height = self.origin_height;
        self.dynamic_image = Some(tp.to_owned());
        Ok(())
    }

    pub fn load_clear_texture(&mut self) -> MoveMakerResult<()> {
        //默认填充是黑色
        let canvas = DynamicImage::new_rgba8(self.graph_width, self.graph_height);
        self.dynamic_image = Some(canvas);
        Ok(())
    }
}
