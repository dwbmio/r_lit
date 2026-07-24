//! deskpet library — Bevy 桌面宠物 + loopback RPC。
//!
//! 可被 starlink-dev 等同进程嵌入：`launch_with_inbox` 在调用方线程跑 Bevy，
//! `call_via_inbox` 在同进程内走 RpcTaskInbox，无需 TCP。

mod app;
mod assets_path;
mod error;
mod notify;
mod os_notify;
pub mod rpc;
mod pet_render;
mod rpc_methods;
mod tray_icon;

mod in_process;

pub use app::{LaunchConfig, launch_with_inbox, try_cli};
pub(crate) use app::{Mascot, PetWin, Screen, Settings, Walk};
pub use in_process::call_via_inbox;
pub use rpc::bevy_bridge::RpcTaskInbox;
