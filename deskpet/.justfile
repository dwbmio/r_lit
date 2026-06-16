set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
<<<<<<<< HEAD:ui-trim/.justfile
set dotenv-load

CARGO_PROJ_OUTPUT_BIN := "ui-trim"
PY_SHEBANG := if os() == "windows"{"python"} else {"/usr/bin/env python"}
BINARY_INSTALL_PATH := if os() == "macos"{"/usr/local/bin"} else {"D://dtool"}
========

set dotenv-load

PROJ_NAME := ""
CARGO_PROJ_OUTPUT_BIN := "deskpet"
PY_SHEBANG := if os() == "windows"{"python"} else {"/usr/bin/env python"}
BINARY_INSTALL_PATH := if os() == "macos"{"/usr/local/bin"} else {"D://dtool"}
PLAT_FORMS := "aarch64-apple-darwin"
>>>>>>>> feat/deskpet:deskpet/.justfile

[private]
default:
    just --list

__cargo_build method plat="":
    #!{{PY_SHEBANG}}
    import os
    import sys
    target = "--target {{plat}}" if "{{plat}}" else ""
    release = "--release" if "{{method}}" == "release" else ""
    cmd = "cargo build %s %s" % (target, release)
    print(cmd)
    ret = os.system(cmd)
    if ret != 0:
        sys.exit(2)

test:
    cargo test

__install_bin method:
    #!{{PY_SHEBANG}}
    import os
    import shutil
    import sys
    bin_f = sys.platform == "win32" and "{{CARGO_PROJ_OUTPUT_BIN}}.exe" or "{{CARGO_PROJ_OUTPUT_BIN}}"
    mv_f = os.path.join(r'{{justfile_directory()}}', 'target', '{{method}}' == "release" and "release" or "debug", bin_f)
    if not os.path.isfile(mv_f):
        print("cargo build failed!")
        sys.exit(2)
    shutil.copyfile(mv_f, "{{BINARY_INSTALL_PATH}}/{{CARGO_PROJ_OUTPUT_BIN}}")
    os.system("sudo chmod +x {{BINARY_INSTALL_PATH}}/{{CARGO_PROJ_OUTPUT_BIN}}")

install_loc method="release":
    just __cargo_build {{method}}
    sudo just __install_bin {{method}}

gen_doc:
    git-cliff -o ./CHANGE_LOG.md
