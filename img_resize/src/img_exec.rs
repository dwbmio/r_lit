use image::{GenericImageView, ImageFormat, ImageOutputFormat};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use crate::re_tp::tp::ReError;

fn rand_filename() -> String {
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(12)
        .map(char::from)
        .collect();

    rand_string
}

pub fn convert_tp(
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

pub fn re_tp(
    tp: &PathBuf,
    size: (u32, u32),
    out: Option<PathBuf>,
    is_thumb: bool,
    mine_type: &str,
) -> Result<(), ReError> {
    let out_i = out.unwrap_or(std::env::current_dir().expect("current dir get failed!"));
    let im = image::open(tp).unwrap();
    //thumb ignore when max > w && max > h
    if is_thumb {
        if size.0 >= im.width() && size.1 >= im.height() {
            drop(im);
            return Ok(());
        }
    }
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
    // thumb the tp
    fs::create_dir_all(out_i)?;
    let ran_fname = OsString::from(rand_filename().to_owned());
    let f_name: &std::ffi::OsStr = tp.file_name().unwrap_or(&(ran_fname));
    let im_r: image::DynamicImage = match is_thumb {
        true => im.thumbnail(size.0, size.1),
        false => im.resize_exact(size.0, size.1, image::imageops::FilterType::CatmullRom),
    };
    let f_path: &Path = Path::new(f_name);
    let fo = &mut std::fs::File::create(f_path)?;
    let out = ImageOutputFormat::from(
        ImageFormat::from_mime_type(mine_type).expect("unknown output format!"),
    );
    im_r.write_to(fo, out)?;
    Ok(())
}
