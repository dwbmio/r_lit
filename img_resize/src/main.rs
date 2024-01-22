use clap::{arg, command, value_parser, ArgAction, ArgGroup, Command};
use re_tp::tp::{self, ReError, TpResize};

use core::panic;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::exit;
mod re_tp;

fn handle(pb: &str) {
    let tar = Path::new(pb);
    if tar.exists() {
        panic!("path is not exists!")
    }
}

fn main() -> Result<(), ReError> {
    let cli = command!() // requires `cargo` feature
        .arg(
            arg!(
                -m --max_pixel <MAX_WIDTH> "Set the MAX-WIDTH to filter the textue."

            )
            // We don't have syntax yet for optional options, so manually calling `required`
            .value_parser(value_parser!(u32)),
        )
        .arg(
            arg!(
                --rw <RESIZE_WIDTH> "Set the MAX-WIDTH to filter the textue."
            )
            // We don't have syntax yet for optional options, so manually calling `required`
            .value_parser(value_parser!(u32))
            .conflicts_with("max_pixel"),
        )
        .arg(
            arg!(
                --rh <RESIZE_HEIGHT> "Set the MAX-HEIGHT to filter the textue."
            )
            // We don't have syntax yet for optional options, so manually calling `required`
            .value_parser(value_parser!(u32))
            .conflicts_with("max_pixel"),
        )
        .arg(
            arg!(
                <path> "Single *.(png|jpg) path or just a path."
            )
            .required(true)
            .value_parser(value_parser!(std::path::PathBuf)),
        )
        .group(
            ArgGroup::new("filter")
                .args(["rw", "rh"])
                .multiple(true)
                .requires_all(["rw", "rh"])
                .conflicts_with("max_pixel"),
        )
        .get_matches();

    let mut tp_handle = TpResize {
        max_pixel: 0,
        height: 0,
        width: 0,
        tp: PathBuf::new(),
        out: None,
    };
    if let Some(mw) = cli.get_one::<u32>("max_pixel") {
        tp_handle.max_pixel = *mw;
    };

    if let Some(tp) = cli.get_one::<PathBuf>("path") {
        tp_handle.tp = tp.to_path_buf();
    };

    if let Some(mw) = cli.get_one::<u32>("rw") {
        tp_handle.width = *mw;
    };

    if let Some(tp) = cli.get_one::<u32>("rh") {
        tp_handle.height = *tp;
    };

    tp_handle.exec()?;
    Ok(())
}

// fn main() {
//     let opt = Opts::parse();
//     let cf = fs::read_to_string(opt.config).unwrap_or_else(|_| {
//         println!("Load config failed!Create the config first");
//         exit(2)
//     });
//     let out = YamlLoader::load_from_str(cf.as_str()).unwrap_or_else(|_| {
//         println!("Load config failed!Not in yaml fmt");
//         exit(2)
//     });
//     let c = &out[0].to_owned();
//     let im = image::open(&opt.image).unwrap();
//     println!(
//         "load texture from {:}, dimensions={:?} color={:?}",
//         &opt.image,
//         im.dimensions(),
//         im.color()
//     );

//     // The color method returns the image's ColorType
//     println!("{:?}", im.color());
//     let o_s = c["vec_size"].as_vec().unwrap();
//     let o_f = c["vec_f"].as_vec().unwrap();
//     let is = c["base_f"].is_badvalue();
//     let mut base_f = "";
//     if !is {
//         base_f = c["base_f"].as_str().unwrap();
//     }
//     let mut idx = 0;
//     for o in o_s {
//         let f = o_f.get(idx).unwrap().as_str().unwrap();
//         let f_p =
//             &Path::new(base_f).join(f.replace("/", std::path::MAIN_SEPARATOR.to_string().as_str()));
//         if let Some(f_pp) = f_p.parent() {
//             if !f_pp.is_dir() {
//                 let _ = fs::create_dir_all(f_pp);
//             }
//         }
//         let fo = &mut File::create(f_p).unwrap();
//         let fo_size = (o[0].as_i64().unwrap() as u32, o[1].as_i64().unwrap() as u32);
//         println!(
//             "output file:{} <size->w={}, h={}>",
//             f_p.as_path().to_str().unwrap(),
//             fo_size.0,
//             fo_size.1
//         );
//         let im_r = im.thumbnail(fo_size.0, fo_size.1);
//         let _ = im_r.write_to(fo, ImageFormat::Png);
//         idx += 1;
//     }
// }
