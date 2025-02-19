pub mod error;
pub mod ffmpeg_inc;
pub mod stage;
mod viewport;
use ffmpeg_inc::{stream::Streams, texture::Texture};
use image::DynamicImage;
use std::{
    collections::HashMap, path::{Path, PathBuf}, time::{Duration, SystemTime, UNIX_EPOCH}
};
use viewport::ViewPort;

type MoveMakerResult<T> = Result<T, error::MovieError>;

#[derive(Debug)]
enum VideoFmtType {
    Mp4,
}

impl Default for VideoFmtType {
    fn default() -> Self {
        VideoFmtType::Mp4 // 指定 VariantA 为默认类型
    }
}

#[allow(unused)]
pub struct RuntimeCtx {
    pub init: bool,
    // 静态资源初始化路径
    source_path: Option<PathBuf>,
    video_fmt: VideoFmtType,
    view_port: ViewPort,
    stream: Streams,
    // 所有用到的纹理 按下标放到数组里
    pub textures: HashMap<String, Texture>,
    pub index_catch:u32,
    pub draw_call_times: u64
}

impl RuntimeCtx {
    pub fn new(width: u32, height: u32, secs: u64, fps: u32) -> Self {
        Self {
            init: false,
            source_path: None,
            video_fmt: VideoFmtType::default(),
            view_port: ViewPort {
                height: height,
                width: width,
            },
            stream: Streams {
                duration: Duration::from_secs(secs),
                fps,
            },
            textures: HashMap::new(),
            index_catch: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards").as_millis() as u32,
            draw_call_times: 0
        }
    }

    ///初始化ffmpeg上下文
    pub fn init(&mut self, proj_path: Option<PathBuf>) -> crate::MoveMakerResult<()> {
        if self.init {
            ffmpeg_inc::init_env()?;
        }
        if let Some(p) = proj_path {
            self.source_path = Some(p);
        } else {
            self.source_path = Some(Path::new(".").to_path_buf());
        }
        Ok(())
    }

    ///
    /// 设置文件搜索路径
    pub fn set_source_path(&mut self, path: PathBuf) {
        self.source_path = Some(path);
    }

    pub fn load_loc_image<'a>(&mut self, rel_path: &str, id: &str) -> MoveMakerResult<String> {
        let t_pf = self
            .source_path
            .clone()
            .expect("Ensure111")
            .join(rel_path)
            .to_path_buf();
        // gen id by len
        let id = id.to_owned();
        let mut frame = Texture::new(&id);
        println!("load loc image:[{}]{:?}", id, t_pf);
        frame.load_texture(&t_pf)?;
        self.textures.insert(id.to_string(), frame);
        Ok(id)
    }

    pub fn set_textures_cache(&mut self, img: &DynamicImage, name: &str) -> MoveMakerResult<String> {
        // let start = SystemTime::now();
        // let since_the_epoch = start
        //     .duration_since(UNIX_EPOCH)
        //     .expect("Time went backwards");
        // let tp_id = (since_the_epoch.as_millis() as u32).to_string();
        self.index_catch = self.index_catch + 1;
        let tp_id = self.index_catch.to_string();
        let mut frame = Texture::new_with_name(&tp_id, name);
        println!("cache image:[{}] name:{} ", tp_id, name);
        frame.set_texture(img)?;
        let res = self.textures.insert(tp_id.clone(),frame);

        Ok(tp_id)
    }

    ///
    /// 根据id返回纹理数据
    pub fn get_texture(&self, id: &str) -> &Texture {
        &self.textures.get(id).expect(format!("unload texture id={:?} yet", id).as_str())
    }

    ///
    /// 根据纹理名称返回数据
    pub fn get_texture_by_name(&self, name: &str) -> Option<&Texture> {
        for (_, t) in &self.textures {
            if let Some(n) = &t.name  {
                if n == name {
                    return Some(t);
                }
            }
        }
        None
    }   
}
