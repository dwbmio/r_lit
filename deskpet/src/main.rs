// On Windows release builds, don't pop up a console window behind the mascot.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = deskpet::try_cli(&args) {
        std::process::exit(code);
    }
    deskpet::launch_with_inbox(deskpet::RpcTaskInbox::new(), deskpet::LaunchConfig::default());
}
