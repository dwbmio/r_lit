use clap::{Parser, YamlLoader};
use std::fs;
use std::process::{exit};
use image::{GenericImageView, ImageFormat};
use std::fs::File;
use std::path::Path;

#[derive(Parser)]
#[clap(version = "1.0", author = "dwb<dwb@dwb.ren>")]
struct Opts {
    #[clap(short, long, default_value = "config.yaml")]
    config: String,

    #[clap(required = true)]
    image: String,
}

fn main() {
    let opt = Opts::parse();
    let cf = fs::read_to_string(opt.config).unwrap_or_else(|_| {
        println!("Load config failed!Create the config first");
        exit(2)
    });
    let out = YamlLoader::load_from_str(cf.as_str()).unwrap_or_else(|_| {
        println!("Load config failed!Not in yaml fmt");
        exit(2)
    });
    let c = &out[0].to_owned();
    let im = image::open(&opt.image).unwrap();
    println!("load texture from {:}, dimensions={:?} color={:?}", &opt.image, im.dimensions(), im.color());

    // The color method returns the image's ColorType
    println!("{:?}", im.color());
    let o_s = c["vec_size"].as_vec().unwrap();
    let o_f = c["vec_f"].as_vec().unwrap();
    let is = c["base_f"].is_badvalue();
    let mut base_f = "";
    if !is {
        base_f = c["base_f"].as_str().unwrap();
    }
    let mut idx = 0;
    for o in o_s {
        let f = o_f.get(idx).unwrap().as_str().unwrap();
        let f_p = &Path::new( base_f).join(f.replace("/", std::path::MAIN_SEPARATOR.to_string().as_str()));
        if let Some(f_pp) = f_p.parent(){
            if !f_pp.is_dir(){
                let _ = fs::create_dir_all(f_pp);
            }
        }
        let fo = &mut File::create(f_p).unwrap();
        let fo_size = (o[0].as_i64().unwrap() as u32, o[1].as_i64().unwrap() as u32);
        println!("output file:{} <size->w={}, h={}>", f_p.as_path().to_str().unwrap(), fo_size.0, fo_size.1);
        let im_r = im.thumbnail(fo_size.0, fo_size.1);
        let _= im_r.write_to(fo, ImageFormat::Png);
        idx += 1;
    }
}