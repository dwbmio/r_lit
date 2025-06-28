use crate::error::ReError;
use crate::subcmd::SubExecutor;
use clap::ArgMatches;
use infer::MatcherType;
use walkdir::WalkDir;
use yaml_rust::YamlLoader;

#[derive(Debug, Default, Clone)]
pub enum ActionType {
    Resize,
    Convert,
    #[default]
    None,
}
use image::{GenericImageView, ImageFormat, ImageOutputFormat};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

fn rand_filename() -> String {
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(12)
        .map(char::from)
        .collect();

    rand_string
}

fn convert_tp(
    tp: &PathBuf,
    convert_type: ImageFormat,
    out: Option<PathBuf>,
) -> Result<(), ReError> {
    // convert to .jpg ext
    let ran_fname = OsString::from(rand_filename().to_owned());
    let f_name: &std::ffi::OsStr = tp.file_name().unwrap_or(&(ran_fname));
    let out_i = out.unwrap_or(std::env::current_dir().expect("current dir get failed!"));
    fs::create_dir_all(out_i)?;

    let im = image::open(tp)?;
    let f_path: &Path = Path::new(f_name);
    let convert_ext = clap::builder::OsStr::from(convert_type.extensions_str()[0]);
    let f_j_path = f_path.with_extension(convert_ext);
    let fo = &mut std::fs::File::create(f_j_path)?;
    im.write_to(fo, ImageOutputFormat::Jpeg(100))?;
    Ok(())
}

fn re_tp(
    tp: &PathBuf,
    size: (u32, u32),
    out: Option<PathBuf>,
    is_thumb: bool,
    mine_type: &str,
) -> Result<(), ReError> {
    let out_i = out.unwrap_or(std::env::current_dir().expect("current dir get failed!"));
    let im = image::open(tp)?;
    //thumb ignore when max > w && max > h
    if is_thumb {
        if size.0 >= im.width() && size.1 >= im.height() {
            drop(im);
            return Ok(());
        }
    }
    log::info!(
        "resize texture from {:?}, pixel={:?} fmt={:?} => {:?}",
        &tp,
        im.dimensions(),
        im.color(),
        if is_thumb {
            format!("max size={:?}", size.0)
        } else {
            format!("size={:?}", size)
        }
    );
    // thumb the tp
    fs::create_dir_all(out_i)?;
    let ran_fname = OsString::from(rand_filename().to_owned());
    let f_name: &std::ffi::OsStr = tp.file_name().unwrap_or(&(ran_fname));
    let im_r: image::DynamicImage = match is_thumb {
        true => im.resize(size.0, size.1, image::imageops::FilterType::Nearest),
        false => im.resize_exact(size.0, size.1, image::imageops::FilterType::CatmullRom),
    };
    let f_path: &Path = Path::new(f_name);
    let fo = &mut std::fs::File::create(f_path)?;
    let out = ImageOutputFormat::from(
        ImageFormat::from_mime_type(mine_type).expect("unknown output format!"),
    );
    log::info!("output fmt={:?}", out);
    im_r.write_to(fo, out)?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
struct RtpExecutor {
    max_pixel: u32,
    height: u32,
    width: u32,
    force_jpg: bool,
    tp: PathBuf,
    out: Option<PathBuf>,
    action: ActionType,
}

impl SubExecutor for RtpExecutor {
    async fn exec(&self, m: &clap::ArgMatches) -> Result<(), ReError> {
        if let Some(c) = m.get_one::<PathBuf>("resize_config") {
            exec_from_config(self.tp.clone(), c.clone())?;
        } else {
            self.exec_resize().await?;
        }
        Ok(())
    }

    async fn single_tp(
        &self,
        path: &PathBuf,
        out: Option<PathBuf>,
    ) -> Result<(), crate::error::ReError> {
        let is_thumb = self.max_pixel > 0;
        let kind = infer::get_from_path(path)?;
        if let Some(k) = kind {
            if k.matcher_type() != MatcherType::Image {
                // ignroe un-image file
                return Ok(());
            }
            match self.action {
                ActionType::Resize => {
                    re_tp(
                        path,
                        (
                            if is_thumb { self.max_pixel } else { self.width },
                            if is_thumb {
                                self.max_pixel
                            } else {
                                self.height
                            },
                        ),
                        out.clone(),
                        is_thumb,
                        k.mime_type(),
                    )?;
                }
                ActionType::Convert => {
                    convert_tp(path, ImageFormat::Jpeg, out.clone())?;
                }
                ActionType::None => {
                    panic!("unknown action to handle tp!")
                }
            }

            return Ok(());
        }
        log::warn!("[warn]unknown file type...ignore!{:?}", path.display());
        Ok(())
    }
}

impl RtpExecutor {
    pub async fn exec_resize(&self) -> Result<(), ReError> {
        if !self.tp.exists() {
            panic!("path not exists!");
        }
        log::info!("resize :{} => {:?}", self.tp.display(), self.out);
        match self.tp.is_file() {
            true => self.single_tp(&self.tp, self.out.to_owned()).await,
            false => self.walk(&self.tp, self.out.to_owned()).await,
        }
    }
}

fn exec_from_config(img_tp: PathBuf, pp: PathBuf) -> Result<(), ReError> {
    let c = fs::read_to_string(pp).expect("Load config failed!Create the config first");
    let out = YamlLoader::load_from_str(c.as_str()).expect("Load config failed!Not in yaml fmt");
    let c = &out[0].to_owned();
    let o_s = c["vec_size"].as_vec().expect("`vec_size` is not vec!");
    let o_f = c["vec_f"].as_vec().expect("`vec_f` is not vec!");
    let is = c["base_f"].is_badvalue();
    let mut base_f = "";
    if !is {
        base_f = c["base_f"].as_str().expect("`base_f` not set to  string!");
    }
    let im = image::open(&img_tp)
        .expect(format!("`image open filed!=>>{:?}`", &img_tp.display()).as_str());
    let mut idx = 0;
    for o in o_s {
        let f = o_f.get(idx).unwrap().as_str().unwrap();
        let f_p = &std::path::Path::new(base_f)
            .join(f.replace("/", std::path::MAIN_SEPARATOR.to_string().as_str()));
        if let Some(f_pp) = f_p.parent() {
            if !f_pp.is_dir() {
                let _ = fs::create_dir_all(f_pp);
            }
        }
        let fo = &mut fs::File::create(f_p).unwrap();
        let fo_size = (
            o[0].as_i64()
                .expect(format!("conver from :{:?} to u32 failed", o[0]).as_str())
                as u32,
            o[1].as_i64()
                .expect(format!("conver from :{:?} to u32 failed", o[1]).as_str())
                as u32,
        );
        log::info!(
            "output file:{} <size->w={}, h={}>",
            f_p.as_path().to_str().unwrap(),
            fo_size.0,
            fo_size.1
        );
        let im_r = im.thumbnail(fo_size.0, fo_size.1);
        let _ = im_r.write_to(fo, ImageFormat::Png);
        idx += 1;
    }

    Ok(())
}

pub async fn exec(m: &ArgMatches) -> Result<(), ReError> {
    let mut tp_inc = RtpExecutor::default();
    if let Some(mw) = m.get_one::<u32>("max_pixel") {
        tp_inc.max_pixel = *mw;
        tp_inc.action = ActionType::Resize;
    }

    if let Some(tp) = m.get_one::<PathBuf>("path") {
        tp_inc.tp = tp.to_path_buf();
    }

    if let Some(tp) = m.get_one::<bool>("force_jpg") {
        tp_inc.force_jpg = *tp;
        tp_inc.action = ActionType::Convert;
    }

    if let Some(mw) = m.get_one::<u32>("rw") {
        tp_inc.width = *mw;
        tp_inc.action = ActionType::Resize;
    }

    if let Some(tp) = m.get_one::<u32>("rh") {
        tp_inc.height = *tp;
        tp_inc.action = ActionType::Resize;
    }
    tp_inc.exec_resize().await
}
