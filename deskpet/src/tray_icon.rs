//! 菜单栏图标：从磁盘同步预加载，避免 Bevy 异步 asset 导致图标为空。
use std::path::Path;

use bevy::asset::{Assets, Handle, RenderAssetUsages};
use bevy::image::{Image, ImageFormat, ImageSampler, ImageType};
use bevy::prelude::*;

/// 将 `tray.png` 读入 `Assets<Image>` 并保留 CPU 像素（供 tray-icon 读取）。
pub fn preload_tray_png(path: &Path, images: &mut Assets<Image>) -> Result<Handle<Image>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let image = Image::from_buffer(
        &bytes,
        ImageType::Format(ImageFormat::Png),
        Default::default(),
        true,
        ImageSampler::nearest(),
        RenderAssetUsages::MAIN_WORLD,
    )
    .map_err(|e| format!("decode: {e}"))?;
    Ok(images.add(image))
}
