set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# 这里需要本地有.env文件
set dotenv-load
# ===============
# ===Variables===
PROJ_NAME := ""
CARGO_PROJ_OUTPUT_BIN := "bulk_upload"
PY_SHEBANG := if os() == "windows"{"python"} else {"/usr/bin/env python"}
BINARY_INSTALL_PATH := if os() == "macos"{"/usr/local/bin"} else {"D://dtool"}
PLAT_FORMS := "aarch64-apple-darwin"
# ===Variables===
# ===============


# =============
# ===Private===
[private]
default:
    just --list


__cargo_build method plat="":
    #!{{PY_SHEBANG}}
    import sys
    import os
    exec_cmd = "cargo build %s %s" % (len("{{plat}}") > 0 and "--target %s" % "{{plat}}" or "", '{{method}}' == "release" and "--release" or "")
    print(exec_cmd)
    ret = os.system(exec_cmd)
    if ret != 0:
        print("cargo build [%s] failed!" % "{{plat}}")
        sys.exit(2)
    print("cargo build [%s] success!" % "{{plat}}")


__install_bin method:
    #!{{PY_SHEBANG}}
    import os
    import sys
    import shutil

    bin_f = sys.platform == "win32" and "{{CARGO_PROJ_OUTPUT_BIN}}.exe" or "{{CARGO_PROJ_OUTPUT_BIN}}"
    mv_f = os.path.join(r'{{justfile_directory()}}', 'target', '{{method}}' == "release" and "release" or "debug", bin_f)
    print(mv_f)
    if not os.path.isfile(mv_f):
        print("cargo build failed!")
        sys.exit(2)
    shutil.copyfile(mv_f, "{{BINARY_INSTALL_PATH}}/{{CARGO_PROJ_OUTPUT_BIN}}")
    os.system("sudo chmod +x {{BINARY_INSTALL_PATH}}/{{CARGO_PROJ_OUTPUT_BIN}}")

# ===Private===
# =============

install_loc method="release":
    just __cargo_build {{method}}
    sudo just __install_bin {{method}}

# 生成文档
gen_doc:
    git-cliff  -o ./CHANGE_LOG.md
