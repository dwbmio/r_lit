set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# 这里需要本地有.env文件，并且记录的文件内容包括GITHUB_TOKNE
set dotenv-load
# ===============
# ===Variables===
PROJ_NAME := ""
CARGO_PROJ_REL_DIR := "img_resize"
PY_SHEBANG := if os() == "windows"{"python"} else {"/usr/bin/env python"}
BINARY_INSTALL_PATH := if os() == "macos"{"/usr/local/bin"} else {"D://dtool"}
# ===Variables===
# ===============


# =============
# ===Private===
[private]
default:
    just --list


__cargo_build method tar:
    #!{{PY_SHEBANG}}
    import os
    import sys

    exec_cmd ="cargo build %s" % ('{{method}}' == "release" and "--release" or "")
    print(exec_cmd)
    os.system(exec_cmd)



__mv_loc method tar:
    #!{{PY_SHEBANG}}
    import os
    import sys
    import platform
    import shutil

    bin_f = sys.platform == "win32" and "{{tar}}.exe" or "{{tar}}"
    mv_f = os.path.join(r'{{justfile_directory()}}', 'target', '{{method}}' == "release" and "release" or "debug", bin_f)
    if not os.path.isfile(mv_f):
        print("cargo build failed!")
        sys.exit(2)
    shutil.copyfile(mv_f, os.path.join(r'{{BINARY_INSTALL_PATH}}', bin_f))
    print("suc!")

# ===Private===
# =============

install_loc method="release" tar="hfrog-cli":
    just __cargo_build {{method}} {{tar}}
    just __mv_loc {{method}} {{tar}}

# 生成文档
gen_doc:
    git-cliff  -o ./CHANGE_LOG.md



#发布binary页面到hfrog-cli
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
