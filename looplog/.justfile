set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

[private]
default:
    just --list

__cargo_build method="release":
    cargo build --{{method}}

install_loc method="release":
    cargo install --path . --{{method}}
