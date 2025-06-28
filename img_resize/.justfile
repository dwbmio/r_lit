set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# 这里需要本地有.env文件，并且记录的文件内容包括GITHUB_TOKNE
set dotenv-load
# ===============
# ===Variables===
PROJ_NAME := ""
CARGO_PROJ_REL_DIR := "img_resize"
PY_SHEBANG := if os() == "windows"{"python"} else {"/home/dwb/.pyenv/shims python"}
BINARY_INSTALL_PATH := if os() == "windows" {
    "D://dtool"
} else if os() == "macos" {
    "/usr/local/bin"
} else {
    "/home/dwb/data/dtools"
}
# ===Variables===
# ===============


# =============
# ===Private===
[private]
default:
    just --list


__cargo_build method tar:
    #!/usr/bin/env sh
    exec_cmd="cargo build $( [ "{{method}}" = "release" ] && echo "--release" )"
    echo "$exec_cmd"
    eval "$exec_cmd"



__mv_loc method tar:
    #!/usr/bin/sh
    os=$(uname)
    bin_f="$( [ "$os" = "windows" ] && echo "{{tar}}.exe" || echo "{{tar}}" )"
    build_dir="$( [ "$os" = "windows" ] && echo "target\\{{method}}" || echo "target/{{method}}" )"
    mv_f="$( [ "$os" = "windows" ] && echo "{{justfile_directory()}}\\target\\{{method}}\\${bin_f}" || echo "{{justfile_directory()}}/target/{{method}}/${bin_f}" )"
    dest="$( [ "$os" = "windows" ] && echo "{{BINARY_INSTALL_PATH}}\\${bin_f}" || echo "{{BINARY_INSTALL_PATH}}/${bin_f}" )"
    if [ ! -f "$mv_f" ]; then
        echo "cargo build failed!"
        exit 2
    fi
    cp "$mv_f" "$dest"
    echo "suc!"


# ===Private===
# =============

install_loc method="release" tar="img_resize":
    just __cargo_build {{method}} {{tar}}
    just __mv_loc {{method}} {{tar}}

# 生成文档
gen_doc:
    git-cliff  -o ./CHANGE_LOG.md



#发布binary页面到hfrog
__pub_release:
    hfrog -p {{justfile_directory()}}/out publish --alias-method dry


# 生成输出的路径
__gen_outdir tar:
    #!{{PY_SHEBANG}}
    import os
    import sys
    import shutil
    bin_f = sys.platform == "win32" and "{{tar}}.exe" or "{{tar}}"
    mv_f = os.path.join(r'{{justfile_directory()}}', 'target', "release", bin_f)

    t = os.path.join(r'{{justfile_directory()}}', "out")
    if not os.path.isdir(t):
        os.makedirs(t)
    f_f = os.path.join(r'{{justfile_directory()}}', "hfrog.yml")
    t_f = os.path.join(t, "hfrog.yml")
    shutil.copyfile(f_f, t_f)
    shutil.copyfile(mv_f, os.path.join(t, bin_f))
    shutil.copyfile(".hfrog_config", os.path.join(t, ".hfrog_config"))


# 发布img_resize到hfrog
build_and_pub tar="img_resize":
    just __cargo_build release {{tar}}
    just __gen_outdir {{tar}}
    just __pub_release
