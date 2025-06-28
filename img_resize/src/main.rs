use clap::{arg, command, value_parser, ArgGroup};
use std::path::PathBuf;

use crate::error::ReError;
mod error;
mod subcmd;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let c = fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {}] {}",
                humantime::format_rfc3339_seconds(std::time::SystemTime::now()),
                record.level(),
                message
            ))
        })
        // Add blanket level filter -
        .level(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .chain(std::io::stdout())
        .apply()?;
    let mut cli = command!() // requires `cargo` feature
        .subcommand(
            command!("r_resize")
                .about("Resize by pure rust lib(net-less) only dependency loc")
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
                    .value_parser(value_parser!(bool))
                    .conflicts_with_all(["max_pixel", "rw", "rh"]),
                )
                .arg(
                    arg!(
                        -m --max_pixel <MAX_WIDTH> "Set the MAX-WIDTH to filter the textue."
                    )
                    .value_parser(value_parser!(u32)),
                )
                .arg(
                    arg!(
                        --rw <RESIZE_WIDTH> "Set the MAX-WIDTH to filter the textue."
                    )
                    .value_parser(value_parser!(u32))
                    .conflicts_with("max_pixel"),
                )
                .arg(
                    arg!(
                        --rh <RESIZE_HEIGHT> "Set the MAX-HEIGHT to filter the textue."
                    )
                    .value_parser(value_parser!(u32))
                    .conflicts_with("max_pixel"),
                )        .arg(
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
            )
        .subcommand(
            command!("tinyfy")
                .about("Tinyfy texture by tinify API")
                .arg(
                arg!(
                    -d --do_size_perf <DO_SIZE_PERF> "tinypng best redude size ")
                ).arg(
                    arg!(
                        <path> "Single *.(png|jpg) path or just a path."
                    )
                    .required(true)
                    .value_parser(value_parser!(std::path::PathBuf)),
                )
        );

    let matches = cli.clone().get_matches();
    let c = match matches.subcommand() {
        Some(("r_resize", matches)) => subcmd::r_tp::exec(matches).await,
        Some(("tinyfy", matches)) => subcmd::tinify_tp::exec(matches).await,
        _ => cli
            .print_help()
            .map_err(|_| ReError::CustomError("unknown print error".to_owned())),
    };
    if c.is_err() {
        log::error!("{:?}", c);
    }
    Ok(())
}
