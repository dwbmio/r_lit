use clap::{arg, command, value_parser, ArgGroup};
use re_tp::{
    tp::{ActionType, TpResize},
    tp_helper,
};
use std::path::PathBuf;
mod img_exec;
mod re_tp;

fn main() -> Result<(), re_tp::ReError> {
    let cli = command!() // requires `cargo` feature
        .arg(
            arg!(
            -c --resize_config <RESIZE_CONFIG> "Set the resize config for resize the texture.")
            .value_parser(value_parser!(PathBuf))
            .conflicts_with_all(["max_pixel", "rw", "rh"]),
        )
        .arg(
            arg!(
                -j --force_jpg <FORCE_JPG> "Convert the texture format to jpg force."
            )
            // We don't have syntax yet for optional options, so manually calling `required`
            .value_parser(value_parser!(bool))
            .conflicts_with_all(["max_pixel", "rw", "rh"]),
        )
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

    let mut tp_inc = TpResize {
        max_pixel: 0,
        height: 0,
        width: 0,
        tp: PathBuf::new(),
        force_jpg: false,
        out: None,
        action: ActionType::None,
    };
    if let Some(mw) = cli.get_one::<u32>("max_pixel") {
        tp_inc.max_pixel = *mw;
        tp_inc.action = ActionType::Resize
    };

    if let Some(tp) = cli.get_one::<PathBuf>("path") {
        tp_inc.tp = tp.to_path_buf();
    };

    if let Some(tp) = cli.get_one::<bool>("force_jpg") {
        tp_inc.force_jpg = *tp;
        tp_inc.action = ActionType::Convert
    };

    if let Some(mw) = cli.get_one::<u32>("rw") {
        tp_inc.width = *mw;
        tp_inc.action = ActionType::Resize
    };

    if let Some(tp) = cli.get_one::<u32>("rh") {
        tp_inc.height = *tp;
        tp_inc.action = ActionType::Resize
    };

    if let Some(c) = cli.get_one::<PathBuf>("resize_config") {
        tp_helper::exec_from_config(tp_inc.tp, c.clone())?;
    } else {
        tp_inc.exec_resize()?;
    }

    Ok(())
}
