use image::ImageError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    #[error("{0}")]
    ImageHandleError(#[from] ImageError),

    #[error("{0}")]
    WalkDirError(#[from] walkdir::Error),

    #[error("{0}")]
    ParseError(#[from] yaml_rust::EmitError),
}

pub mod tp_helper {
    use image::ImageFormat;
    use std::fs;
    use std::path::PathBuf;
    use yaml_rust::YamlLoader;

    use super::ReError;

    pub fn exec_from_config(img_tp: PathBuf, pp: PathBuf) -> Result<(), ReError> {
        let c = fs::read_to_string(pp).expect("Load config failed!Create the config first");
        let out =
            YamlLoader::load_from_str(c.as_str()).expect("Load config failed!Not in yaml fmt");
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
            println!(
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
}

pub mod tp {
    use crate::img_exec;
    use image::ImageFormat;
    use infer::MatcherType;
    use std::path::PathBuf;

    use walkdir::WalkDir;

    use super::ReError;

    pub struct TpResize {
        pub(crate) max_pixel: u32,
        pub(crate) width: u32,
        pub(crate) height: u32,
        pub(crate) force_jpg: bool,
        pub(crate) tp: PathBuf,
        pub(crate) out: Option<PathBuf>,
        pub(crate) action: ActionType,
    }

    pub enum ActionType {
        Resize,
        Convert,
        None,
    }

    impl TpResize {
        fn single_tp(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<(), ReError> {
            let is_thumb = self.max_pixel > 0;

            let kind = infer::get_from_path(path)?;
            if let Some(k) = kind {
                if k.matcher_type() != MatcherType::Image {
                    // ignroe un-image file
                    return Ok(());
                }

                match self.action {
                    ActionType::Resize => {
                        img_exec::re_tp(
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
                        img_exec::convert_tp(path, ImageFormat::Jpeg, out.clone())?;
                    }
                    ActionType::None => {
                        panic!("unknown action to handle tp!")
                    }
                }

                return Ok(());
            }
            println!("[warn]unknown file type...ignore!{:?}", path.display());
            Ok(())
        }

        fn walk(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<(), ReError> {
            let walker = WalkDir::new(path).into_iter();
            println!("start walk dir :{}...", path.display());
            for entry in walker.filter_entry(|e| !Self::is_hidden(e)) {
                let entry = entry?;
                if entry.path().is_file() {
                    self.single_tp(&entry.path().to_path_buf(), out.clone())?
                }
            }
            Ok(())
        }

        fn is_hidden(entry: &walkdir::DirEntry) -> bool {
            entry
                .file_name()
                .to_str()
                .map(|s| s.starts_with("."))
                .unwrap_or(false)
        }

        pub fn exec_resize(&self) -> Result<(), ReError> {
            if !self.tp.exists() {
                panic!("path not exists!");
            }
            println!("exec ...");
            match self.tp.is_file() {
                true => self.single_tp(&self.tp, self.out.to_owned()),
                false => self.walk(&self.tp, self.out.to_owned()),
            }
        }
    }
}
