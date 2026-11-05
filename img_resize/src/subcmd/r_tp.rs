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
    /// Where the result was written (absent for skipped / failed files).
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
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
) -> Result<PathBuf, ReError> {
    let ran_fname = OsString::from(rand_filename().to_owned());
    let f_name: &std::ffi::OsStr = tp.file_name().unwrap_or(&(ran_fname));
    // Output directory:
    //   1) explicit `out` argument wins,
    //   2) otherwise the *input's* parent dir (in-place overwrite —
    //      matches the README's "overwrites original by default"),
    //   3) finally cwd as a last-resort fallback when the input is a
    //      bare filename with no parent (rare; usually only on
    //      hand-rolled invocations).
    // Pre-fix this default was always cwd, which silently dropped the
    // output anywhere the user happened to be `cd`'d to — and made
    // batch scripts that pass absolute paths impossible to use.
    let out_i = out.unwrap_or_else(|| {
        tp.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_dir().expect("current dir get failed!")
            })
    });
    fs::create_dir_all(&out_i)?;

    let im = image::open(tp)?;
    let convert_ext = clap::builder::OsStr::from(convert_type.extensions_str()[0]);
    let f_j_path = out_i.join(f_name).with_extension(convert_ext);
    let fo = &mut std::fs::File::create(&f_j_path)?;
    im.write_to(fo, ImageOutputFormat::Jpeg(100))?;
    Ok(f_j_path)
}

fn re_tp(
    tp: &PathBuf,
    size: (u32, u32),
    out: Option<PathBuf>,
    is_thumb: bool,
    mine_type: &str,
    json_output: bool,
) -> Result<((u32, u32, u32, u32), PathBuf), ReError> {
    // Same "where do bytes land?" precedence as `convert_tp`:
    // explicit `out` → input's parent dir (in-place overwrite) →
    // cwd. The previous unconditional cwd default landed output
    // wherever the caller happened to be `cd`'d to, which made
    // batch scripts passing absolute paths essentially silent
    // no-ops (input remained untouched, output written to a
    // forgotten directory).
    let explicit_out = out.is_some();
    let out_i = out.unwrap_or_else(|| {
        tp.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    });
    let im = image::open(tp)?;
    let orig_size = im.dimensions();

    if is_thumb {
        if size.0 >= im.width() && size.1 >= im.height() {
            // Already small enough. With an explicit output dir, still place
            // a copy there so the output set is complete.
            let dest = match (explicit_out, tp.file_name()) {
                (true, Some(name)) => {
                    fs::create_dir_all(&out_i)?;
                    let dest = out_i.join(name);
                    fs::copy(tp, &dest)?;
                    dest
                }
                _ => tp.clone(),
            };
            return Ok(((orig_size.0, orig_size.1, orig_size.0, orig_size.1), dest));
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

    fs::create_dir_all(&out_i)?;
    let ran_fname = OsString::from(rand_filename().to_owned());
    let f_name: &std::ffi::OsStr = tp.file_name().unwrap_or(&(ran_fname));
    let im_r: image::DynamicImage = match is_thumb {
        true => im.resize(size.0, size.1, image::imageops::FilterType::Nearest),
        false => im.resize_exact(size.0, size.1, image::imageops::FilterType::CatmullRom),
    };
    let new_size = im_r.dimensions();
    let f_path = out_i.join(f_name);
    let fo = &mut std::fs::File::create(&f_path)?;
    let out = ImageOutputFormat::from(
        ImageFormat::from_mime_type(mine_type).expect("unknown output format!"),
    );

    if !json_output {
        log::info!("output fmt={:?} → {:?}", out, f_path);
    }
    im_r.write_to(fo, out)?;
    Ok(((orig_size.0, orig_size.1, new_size.0, new_size.1), f_path))
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
    async fn single_tp(
        &self,
        path: &PathBuf,
        out: Option<PathBuf>,
    ) -> Result<ProcessResult, ReError> {
        let is_thumb = self.max_pixel > 0;
        let kind = infer::get_from_path(path)?;

        if let Some(k) = kind {
            if k.matcher_type() != MatcherType::Image {
                return Ok(ProcessResult {
                    file: path.display().to_string(),
                    status: "skipped".to_string(),
                    output: None,
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
                            if is_thumb {
                                self.max_pixel
                            } else {
                                self.height
                            },
                        ),
                        out.clone(),
                        is_thumb,
                        k.mime_type(),
                        self.json_output,
                    ) {
                        Ok(((ow, oh, nw, nh), written)) => Ok(ProcessResult {
                            file: path.display().to_string(),
                            status: if (ow, oh) == (nw, nh) { "unchanged" } else { "success" }.to_string(),
                            output: Some(written.display().to_string()),
                            original_size: Some((ow, oh)),
                            new_size: Some((nw, nh)),
                            error: None,
                        }),
                        Err(e) => Ok(ProcessResult {
                            file: path.display().to_string(),
                            status: "failed".to_string(),
                            output: None,
                            original_size: None,
                            new_size: None,
                            error: Some(e.to_string()),
                        }),
                    }
                }
                ActionType::Convert => match convert_tp(path, ImageFormat::Jpeg, out.clone()) {
                    Ok(written) => Ok(ProcessResult {
                        file: path.display().to_string(),
                        status: "converted".to_string(),
                        output: Some(written.display().to_string()),
                        original_size: None,
                        new_size: None,
                        error: None,
                    }),
                    Err(e) => Ok(ProcessResult {
                        file: path.display().to_string(),
                        status: "failed".to_string(),
                        output: None,
                        original_size: None,
                        new_size: None,
                        error: Some(e.to_string()),
                    }),
                },
                ActionType::None => Ok(ProcessResult {
                    file: path.display().to_string(),
                    status: "skipped".to_string(),
                    output: None,
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
                output: None,
                original_size: None,
                new_size: None,
                error: Some("unknown file type".to_string()),
            })
        }
    }

    async fn walk(
        &self,
        path: &PathBuf,
        out: Option<PathBuf>,
    ) -> Result<Vec<ProcessResult>, ReError> {
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
                let result = self
                    .single_tp(&entry.path().to_path_buf(), out.clone())
                    .await?;
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
            return Err(ReError::Input(format!("path does not exist: {}", self.tp.display())));
        }

        if !self.json_output {
            log::info!("resize :{} => {:?}", self.tp.display(), self.out);
        }

        let results = match self.tp.is_file() {
            true => vec![self.single_tp(&self.tp, self.out.to_owned()).await?],
            false => self.walk(&self.tp, self.out.to_owned()).await?,
        };

        for r in &results {
            if let Some(out) = &r.output {
                rlit_cli::add_artifact(out, "image");
            }
        }
        let failed = results.iter().filter(|r| r.status == "failed").count();
        if failed > 0 {
            for r in results.iter().filter(|r| r.status == "failed") {
                log::warn!("failed: {} ({})", r.file, r.error.as_deref().unwrap_or(""));
            }
            rlit_cli::set_failure(
                rlit_cli::ErrorKind::Partial,
                format!("{} of {} file(s) failed", failed, results.len()),
            );
        }
        rlit_cli::set_data(serde_json::json!({
            "total": results.len(),
            "failed": failed,
            "results": results
        }));

        Ok(())
    }
}

fn exec_from_config(img_tp: PathBuf, pp: PathBuf) -> Result<(), ReError> {
    let bad = |what: &str| ReError::Input(format!("resize config {}: {}", pp.display(), what));
    let c = fs::read_to_string(&pp)
        .map_err(|e| ReError::Input(format!("cannot read resize config {}: {}", pp.display(), e)))?;
    let docs = YamlLoader::load_from_str(c.as_str()).map_err(|e| bad(&format!("not valid YAML ({})", e)))?;
    let c = docs.first().ok_or_else(|| bad("empty document"))?;
    let o_s = c["vec_size"].as_vec().ok_or_else(|| bad("`vec_size` must be a list of [w, h]"))?;
    let o_f = c["vec_f"].as_vec().ok_or_else(|| bad("`vec_f` must be a list of output paths"))?;
    if o_s.len() != o_f.len() {
        return Err(bad("`vec_size` and `vec_f` must have the same length"));
    }
    let base_f = if c["base_f"].is_badvalue() {
        ""
    } else {
        c["base_f"].as_str().ok_or_else(|| bad("`base_f` must be a string"))?
    };
    let im = image::open(&img_tp)?;
    let mut results = Vec::new();
    for (o, f) in o_s.iter().zip(o_f) {
        let f = f.as_str().ok_or_else(|| bad("`vec_f` entries must be strings"))?;
        let f_p = std::path::Path::new(base_f)
            .join(f.replace('/', std::path::MAIN_SEPARATOR.to_string().as_str()));
        if let Some(f_pp) = f_p.parent() {
            if !f_pp.as_os_str().is_empty() {
                fs::create_dir_all(f_pp)?;
            }
        }
        let dim = |i: usize| {
            o[i].as_i64()
                .filter(|v| *v > 0)
                .map(|v| v as u32)
                .ok_or_else(|| bad(&format!("size entry {:?} must be [w, h] positive integers", o)))
        };
        let fo_size = (dim(0)?, dim(1)?);
        log::info!("output file:{} <size->w={}, h={}>", f_p.display(), fo_size.0, fo_size.1);
        let im_r = im.thumbnail(fo_size.0, fo_size.1);
        let fo = &mut fs::File::create(&f_p)?;
        im_r.write_to(fo, ImageFormat::Png)?;
        rlit_cli::add_artifact(&f_p, "image");
        results.push(serde_json::json!({
            "output": f_p.display().to_string(),
            "box": [fo_size.0, fo_size.1],
            "new_size": [im_r.width(), im_r.height()],
        }));
    }
    rlit_cli::set_data(serde_json::json!({
        "file": img_tp.display().to_string(),
        "total": results.len(),
        "results": results,
    }));
    Ok(())
}

pub async fn exec(
    path: &PathBuf,
    resize_config: Option<&PathBuf>,
    max_pixel: Option<u32>,
    resize_width: Option<u32>,
    resize_height: Option<u32>,
    force_jpg: bool,
    out_dir: Option<&PathBuf>,
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
        return Err(ReError::Usage(
            "one of --max-pixel, --rw/--rh, --force-jpg or --resize-config is required / 必须指定其中之一".to_string(),
        ));
    };

    let executor = RtpExecutor {
        max_pixel: max_pixel.unwrap_or(0),
        height: resize_height.unwrap_or(0),
        width: resize_width.unwrap_or(0),
        force_jpg,
        tp: path.clone(),
        out: out_dir.cloned(),
        action,
        json_output,
    };

    executor.exec_resize().await
}
