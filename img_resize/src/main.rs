use clap::{arg, command, value_parser, ArgGroup};
use re_tp::tp::{ReError, TpResize};
use std::path::PathBuf;
mod re_tp;

fn main() -> Result<(), ReError> {
    let cli = command!() // requires `cargo` feature
        .arg(
            arg!(
            -c --resize_config <RESIZE_CONFIG> "Set the resize config for resize the texture.")
            .value_parser(value_parser!(PathBuf))
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

    if let Some(c) = cli.get_one::<PathBuf>("resize_config") {
        tp_handle.exec_from_config(c.clone())?;
    } else {
        tp_handle.exec()?;
    }

    Ok(())
}
