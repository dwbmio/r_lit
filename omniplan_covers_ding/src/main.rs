mod ctx;
mod error;
mod subcmd;
use cli_common::clap::{Arg, Command, arg};

fn build_cli() -> Command {
    Command::new("omniplan covert to dingding doc")
        .about("A tool to convert CSV files to specified Excel formats")
        .subcommand(
            Command::new("convert")
                .about("Convert a CSV file to an Excel file")
                .args([
                    Arg::new("csv-file").required(true).help("the csv file "),
                    arg!(-p --parent <String> "Parent task value"),
                    arg!(-t --liter <String> "Liter belong"),
                    arg!(-l --limit <String> "Limit k=v value"),
                    Arg::new("doc-type").required(true).value_parser(
                        cli_common::clap::builder::PossibleValuesParser::new(["task", "require"]),
                    ),
                ]),
        )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = ctx::AppContext::default();
    let mut cmd = build_cli();
    let mts = cmd.clone().get_matches();
    let o = match mts.subcommand() {
        Some(("convert", sub_matches)) => subcmd::convert::handle(sub_matches, &ctx).await,
        _ => {
            cmd.print_help().unwrap();
            Ok(())
        }
    };
    if o.is_err() {
        println!("{:?}", o.err());
    }
    Ok(())
}
