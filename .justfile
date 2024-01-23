set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
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


[macos]
__move_to_server bin_file:
    scp ./{{bin_file}}/target/release/{{bin_file}} dwb@pinyin-ci.bbclient.icu:/Users/dwb/Desktop/ccmd-server/shizi_dev/{{bin_file}}/bin/Darwin
    scp ./scripts/install_{{bin_file}}.sh dwb@pinyin-ci.bbclient.icu:/Users/dwb/Desktop/ccmd-server/shizi_dev/{{bin_file}}/install.sh


__cargo_build bin_file:
    #!{{PY_SHEBANG}}
    import os 
    import time
    PATH_PROJ = os.path.join(r"{{justfile_directory()}}", "{{bin_file}}")
    os.chdir(PATH_PROJ)
    CMD_BUILD = r'''cargo build --release'''
    print(CMD_BUILD, "\n-->>run cmd")
    os.system(CMD_BUILD)

inner_dev_release bin_file plat="macosx64" is_debug="false":
    just __cargo_build {{bin_file}}
    just __move_to_server {{bin_file}}