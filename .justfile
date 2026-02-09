set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

set dotenv-load

# ===============
# ===Variables===
PY_SHEBANG := if os() == "windows"{"python"} else {"/usr/bin/env python"}
# ===Variables===
# ===============


# =============
# ===Private===
[private]
default:
    just --list

# ===Private===
# =============

# 构建指定子项目 (e.g. just build img_resize)
build tar method="release":
    cd {{tar}} && just __cargo_build {{method}}

# 本地安装指定子项目
install_loc tar method="release":
    cd {{tar}} && just install_loc {{method}}
