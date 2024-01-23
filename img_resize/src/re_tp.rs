pub mod tp {
    use clap::builder::OsStr;
    use image::{GenericImageView, ImageError, ImageFormat, ImageOutputFormat};
    use infer::MatcherType;
    use rand::{distributions::Alphanumeric, thread_rng, Rng};
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
    };
    use thiserror::Error;
    use walkdir::WalkDir;
    use yaml_rust::YamlLoader;

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
    pub struct TpResize {
        pub(crate) max_pixel: u32,
        pub(crate) width: u32,
        pub(crate) height: u32,
        pub(crate) force_jpg: bool,
        pub(crate) tp: PathBuf,
        pub(crate) out: Option<PathBuf>,
    }

    impl TpResize {
        fn rand_filename() -> String {
            let rand_string: String = thread_rng()
                .sample_iter(&Alphanumeric)
                .take(12)
                .map(char::from)
                .collect();

            rand_string
        }

        fn re_tp(
            tp: &PathBuf,
            size: (u32, u32),
            out: Option<PathBuf>,
            is_thumb: bool,
            is_force_jpg: bool,
            mine_type: &str,
        ) -> Result<(), ReError> {
            let out_i = out.unwrap_or(std::env::current_dir().expect("current dir get failed!"));
            let im = image::open(tp).unwrap();
            //thumb ignore when max > w && max > h

            if is_thumb && !is_force_jpg {
                if size.0 >= im.width() && size.1 >= im.height() {
                    drop(im);
                    return Ok(());
                }
            }

            fs::create_dir_all(out_i)?;
            println!(
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
            let ran_fname = OsString::from(Self::rand_filename().to_owned());
            let f_name = tp.file_name().unwrap_or(&(ran_fname));

            let im_r: image::DynamicImage = match is_thumb {
                true => im.thumbnail(size.0, size.1),
                false => im.resize_exact(size.0, size.1, image::imageops::FilterType::CatmullRom),
            };

            if is_force_jpg {
                // convert to .jpg ext
                println!("force jpg of {:?}", f_name.to_owned());
                let f_path: &Path = Path::new(f_name);
                let f_j_path =
                    f_path.with_extension(OsStr::from(ImageFormat::Jpeg.extensions_str()[0]));
                let fo = &mut std::fs::File::create(f_j_path).unwrap();
                im_r.write_to(fo, ImageOutputFormat::Jpeg(100))?
            } else {
                let f_path: &Path = Path::new(f_name);
                let fo = &mut std::fs::File::create(f_path).unwrap();
                let out = ImageOutputFormat::from(ImageFormat::from_mime_type(mine_type).unwrap());
                im_r.write_to(fo, out)?
            }

            Ok(())
        }

        fn single_tp(&self, path: &PathBuf, out: Option<PathBuf>) -> Result<(), ReError> {
            let is_thumb = self.max_pixel > 0;

            let kind = infer::get_from_path(path)?;
            if let Some(k) = kind {
                if k.matcher_type() != MatcherType::Image {
                    // ignroe un-image file
                    return Ok(());
                }
                Self::re_tp(
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
                    self.force_jpg,
                    k.mime_type(),
                )?;
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

        pub fn exec(&self) -> Result<(), ReError> {
            if !self.tp.exists() {
                panic!("path not exists!");
            }
            println!("exec ...");
            match self.tp.is_file() {
                true => self.single_tp(&self.tp, self.out.to_owned()),
                false => self.walk(&self.tp, self.out.to_owned()),
            }
        }

        pub fn exec_from_config(&self, pp: PathBuf) -> Result<(), ReError> {
            let c = fs::read_to_string(pp).unwrap_or_else(|_| {
                panic!("Load config failed!Create the config first");
            });
            let out = YamlLoader::load_from_str(c.as_str()).unwrap_or_else(|_| {
                panic!("Load config failed!Not in yaml fmt");
            });
            let c = &out[0].to_owned();
            let o_s = c["vec_size"].as_vec().unwrap();
            let o_f = c["vec_f"].as_vec().unwrap();
            let is = c["base_f"].is_badvalue();
            let mut base_f = "";
            if !is {
                base_f = c["base_f"].as_str().unwrap();
            }
            let im = image::open(&self.tp).unwrap();
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
                let fo_size = (o[0].as_i64().unwrap() as u32, o[1].as_i64().unwrap() as u32);
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
}
