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

[private]
_zpm_install: 
    echo install 

# ===Private===
# =============


[macos]
__move_to_server bin_file:
    cp {{bin_file}} /Users/dwb/Desktop/ccmd-server/shizi_dev/{{bin_file}}/bin/Darwin
    cp ./scripts/install.sh dwb@pinyin-ci.bbclient.icu:/Users/dwb/Desktop/ccmd-server/shizi_dev/{{bin_file}}
    

inner_dev_release tool plat="macosx64" is_debug="false":
    #!{{py_shebang}}
    import os 
    import time
    PATH_PROJ = os.path.join(r"{{justfile_directory()}}", "{{tool}}")
    

    CMD_BUILD = r'''cargo build --release'''.format(cocos_bin = "{{COCOS_BIN}}", path_proj = PATH_PROJ, plat = "{{plat}}", is_debug = "{{is_debug}}", path_build = PATH_BUILD)
    print(CMD_BUILD, "\n-->>run cocos cmd")
    os.system(CMD_BUILD)