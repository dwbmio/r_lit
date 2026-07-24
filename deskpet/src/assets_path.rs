//! 解析 `assets/` 目录：嵌入 starlink-dev serve 时不依赖 CWD。
use std::path::{Path, PathBuf};

/// 环境变量 `DESKPET_ASSETS` 可显式指定 assets 根目录。
pub fn resolve_assets_dir() -> PathBuf {
    if let Ok(p) = std::env::var("DESKPET_ASSETS") {
        let p = PathBuf::from(p);
        if p.join("tray.png").is_file() {
            return p;
        }
    }

    if let Some(home) = home_dir() {
        let share = home.join(".snappywatt/share/deskpet/assets");
        if share.join("tray.png").is_file() {
            return share;
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            for rel in [
                "share/deskpet/assets",
                "../share/deskpet/assets",
                "assets",
            ] {
                let p = parent.join(rel);
                if p.join("tray.png").is_file() {
                    return p;
                }
            }
        }
    }

    // 开发：deskpet 源码树（相对常见 clone 布局）
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            let candidate = ancestor.join("private_work/r_lit/deskpet/assets");
            if candidate.join("tray.png").is_file() {
                return candidate;
            }
            let candidate = ancestor.join("r_lit/deskpet/assets");
            if candidate.join("tray.png").is_file() {
                return candidate;
            }
        }
        let cwd_assets = cwd.join("assets");
        if cwd_assets.join("tray.png").is_file() {
            return cwd_assets;
        }
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("assets")
}

pub fn assets_dir_display(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home = Path::new(&home);
        if let Ok(stripped) = path.strip_prefix(home) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_returns_path_ending_with_assets() {
        let p = resolve_assets_dir();
        assert!(p.ends_with("assets"));
    }
}
