//! Pure-Rust iOS transport: usbmuxd + lockdownd + AFC (idevice crate).
//! Replaces the legacy pymobiledevice3 python glue (tools/ios_diff.py).

use anyhow::{anyhow, Context, Result};
use idevice::afc::opcode::AfcFopenMode;
use idevice::afc::AfcClient;
use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use idevice::IdeviceService;
use std::io::Write;
use std::path::Path;

pub async fn connect_afc() -> Result<AfcClient> {
    let mut mux = UsbmuxdConnection::default()
        .await
        .context("connect usbmuxd (is an iPhone plugged in?)")?;
    let devices = mux.get_devices().await?;
    let dev = devices
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("no iOS device on usbmuxd"))?;
    eprintln!("ios device: udid={}", dev.udid);
    let provider = dev.to_provider(UsbmuxdAddr::default(), "phone_haul");
    AfcClient::connect(&provider)
        .await
        .context("afc connect (device paired & trusted?)")
}

/// Pull a remote file to local path in 4MiB chunks. Returns bytes written.
pub async fn pull_file(afc: &mut AfcClient, remote: &str, local: &Path) -> Result<u64> {
    let mut fd = afc
        .open(remote, AfcFopenMode::RdOnly)
        .await
        .with_context(|| format!("afc open {remote}"))?;
    let mut out = std::fs::File::create(local)?;
    let mut total = 0u64;
    loop {
        let chunk = fd.read_n(4 << 20).await?;
        if chunk.is_empty() {
            break;
        }
        out.write_all(&chunk)?;
        total += chunk.len() as u64;
    }
    fd.close().await?;
    Ok(total)
}

/// Pull Photos.sqlite (+wal/shm) from the device into `dir`; returns main db path.
pub async fn pull_photos_db(afc: &mut AfcClient, dir: &Path) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let main = dir.join("Photos.sqlite");
    for name in ["Photos.sqlite", "Photos.sqlite-wal", "Photos.sqlite-shm"] {
        let dst = dir.join(name);
        match pull_file(afc, &format!("/PhotoData/{name}"), &dst).await {
            Ok(n) => eprintln!("pulled {name}: {n} bytes"),
            Err(e) => {
                if name == "Photos.sqlite" {
                    return Err(e);
                }
                let _ = std::fs::remove_file(&dst); // stale sidecars break wal replay
            }
        }
    }
    main.exists().then(|| main.clone()).context("Photos.sqlite missing")?;
    Ok(main)
}
