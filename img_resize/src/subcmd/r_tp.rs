use crate::error::ReError;
use infer::MatcherType;
use serde::Serialize;
use walkdir::WalkDir;
use yaml_rust::YamlLoader;

#[derive(Debug, Default, Clone)]
pub enum ActionType {
    Resize,
    Convert,
    #[default]
    None,
}

#[derive(Serialize)]
struct ProcessResult {
    file: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_size: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_size: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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
    json_output: bool,
) -> Result<(u32, u32, u32, u32), ReError> {
    let out_i = out.unwrap_or(std::env::current_dir().expect("current dir get failed!"));
    let im = image::open(tp)?;
    let orig_size = im.dimensions();

    if is_thumb {
        if size.0 >= im.width() && size.1 >= im.height() {
            return Ok((orig_size.0, orig_size.1, orig_size.0, orig_size.1));
        }
    }

    if !json_output {
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
    }

    fs::create_dir_all(out_i)?;
    let ran_fname = OsString::from(rand_filename().to_owned());
    let f_name: &std::ffi::OsStr = tp.file_name().unwrap_or(&(ran_fname));
    let im_r: image::DynamicImage = match is_thumb {
        true => im.resize(size.0, size.1, image::imageops::FilterType::Nearest),
        false => im.resize_exact(size.0, size.1, image::imageops::FilterType::CatmullRom),
    };
    let new_size = im_r.dimensions();
    let f_path: &Path = Path::new(f_name);
    let fo = &mut std::fs::File::create(f_path)?;
    let out = ImageOutputFormat::from(
        ImageFormat::from_mime_type(mine_type).expect("unknown output format!"),
    );

    if !json_output {
        log::info!("output fmt={:?}", out);
    }
    im_r.write_to(fo, out)?;
    Ok((orig_size.0, orig_size.1, new_size.0, new_size.1))
}

#[derive(Debug, Clone)]
struct RtpExecutor {
    max_pixel: u32,
    height: u32,
    width: u32,
    force_jpg: bool,
    tp: PathBuf,
    out: Option<PathBuf>,
    action: ActionType,
    json_output: bool,
}

impl RtpExecutor {
    async fn single_tp(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<ProcessResult, ReError> {
        let is_thumb = self.max_pixel > 0;
        let kind = infer::get_from_path(path)?;

        if let Some(k) = kind {
            if k.matcher_type() != MatcherType::Image {
                return Ok(ProcessResult {
                    file: path.display().to_string(),
                    status: "skipped".to_string(),
                    original_size: None,
                    new_size: None,
                    error: Some("not an image file".to_string()),
                });
            }

            match self.action {
                ActionType::Resize => {
                    match re_tp(
                        path,
                        (
                            if is_thumb { self.max_pixel } else { self.width },
                            if is_thumb { self.max_pixel } else { self.height },
                        ),
                        out.clone(),
                        is_thumb,
                        k.mime_type(),
                        self.json_output,
                    ) {
                        Ok((ow, oh, nw, nh)) => Ok(ProcessResult {
                            file: path.display().to_string(),
                            status: "success".to_string(),
                            original_size: Some((ow, oh)),
                            new_size: Some((nw, nh)),
                            error: None,
                        }),
                        Err(e) => Ok(ProcessResult {
                            file: path.display().to_string(),
                            status: "failed".to_string(),
                            original_size: None,
                            new_size: None,
                            error: Some(e.to_string()),
                        }),
                    }
                }
                ActionType::Convert => {
                    match convert_tp(path, ImageFormat::Jpeg, out.clone()) {
                        Ok(_) => Ok(ProcessResult {
                            file: path.display().to_string(),
                            status: "converted".to_string(),
                            original_size: None,
                            new_size: None,
                            error: None,
                        }),
                        Err(e) => Ok(ProcessResult {
                            file: path.display().to_string(),
                            status: "failed".to_string(),
                            original_size: None,
                            new_size: None,
                            error: Some(e.to_string()),
                        }),
                    }
                }
                ActionType::None => Ok(ProcessResult {
                    file: path.display().to_string(),
                    status: "skipped".to_string(),
                    original_size: None,
                    new_size: None,
                    error: Some("no action specified".to_string()),
                }),
            }
        } else {
            if !self.json_output {
                log::warn!("[warn]unknown file type...ignore!{:?}", path.display());
            }
            Ok(ProcessResult {
                file: path.display().to_string(),
                status: "skipped".to_string(),
                original_size: None,
                new_size: None,
                error: Some("unknown file type".to_string()),
            })
        }
    }

    async fn walk(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<Vec<ProcessResult>, ReError> {
        let walker = WalkDir::new(path).into_iter();
        if !self.json_output {
            log::debug!("start walk dir :{}...", path.display());
        }

        let mut results = Vec::new();
        for entry in walker.filter_entry(|e| !Self::is_hidden(e)) {
            if !self.json_output {
                log::debug!("entry:{:?}", entry);
            }
            let entry = entry?;
            if entry.path().is_file() {
                let result = self.single_tp(&entry.path().to_path_buf(), out.clone()).await?;
                results.push(result);
            }
        }
        Ok(results)
    }

    fn is_hidden(entry: &walkdir::DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }

    pub async fn exec_resize(&self) -> Result<(), ReError> {
        if !self.tp.exists() {
            return Err(ReError::CustomError("path not exists!".to_string()));
        }

        if !self.json_output {
            log::info!("resize :{} => {:?}", self.tp.display(), self.out);
        }

        let results = match self.tp.is_file() {
            true => vec![self.single_tp(&self.tp, self.out.to_owned()).await?],
            false => self.walk(&self.tp, self.out.to_owned()).await?,
        };

        if self.json_output {
            let summary = serde_json::json!({
                "total": results.len(),
                "results": results
            });
            println!("{}", serde_json::to_string(&summary)?);
        }

        Ok(())
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

pub async fn exec(
    path: &PathBuf,
    resize_config: Option<&PathBuf>,
    max_pixel: Option<u32>,
    resize_width: Option<u32>,
    resize_height: Option<u32>,
    force_jpg: bool,
    json_output: bool,
) -> Result<(), ReError> {
    if let Some(config) = resize_config {
        exec_from_config(path.clone(), config.clone())?;
        return Ok(());
    }

    let action = if force_jpg {
        ActionType::Convert
    } else if max_pixel.is_some() || (resize_width.is_some() && resize_height.is_some()) {
        ActionType::Resize
    } else {
        return Err(ReError::CustomError(
            "必须指定 --max_pixel 或 --rw/--rh 或 --force_jpg".to_string(),
        ));
    };

    let executor = RtpExecutor {
        max_pixel: max_pixel.unwrap_or(0),
        height: resize_height.unwrap_or(0),
        width: resize_width.unwrap_or(0),
        force_jpg,
        tp: path.clone(),
        out: None,
        action,
        json_output,
    };

    executor.exec_resize().await
}
