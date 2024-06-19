set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# 这里需要本地有.env文件，并且记录的文件内容包括GITHUB_TOKNE
set dotenv-load  
# ===============
# ===Variables===
PY_SHEBANG := if os() == "windows"{"python"} else {"/usr/bin/env python"}
DPM_INSTALL_PATH := if os() == "macos"{
'/usr/local/bin'
} else {
'D://dtool'
}
# ===Variables===
# ===============

# =============
# ===Private===
[private]
default:
    just --list

[windows]
__build_cargo method tar: 
    #!{{PY_SHEBANG}}
    import os 
    import sys 
    import platform
    import shutil

    os.chdir('{{tar}}')
    exec_cmd ="cargo build %s" % ('{{method}}' == "release" and "--release" or "")
    print(exec_cmd)
    os.system(exec_cmd)


[windows]
__mv_loc method tar: 
    #!{{PY_SHEBANG}}
    import os 
    import sys 
    import platform
    import shutil

    bin_f = sys.platform == "win32" and "{{tar}}.exe" or "{{tar}}"
    mv_f = os.path.join(r'{{justfile_directory()}}', r'{{tar}}', 'target', '{{method}}' == "release" and "release" or "debug", bin_f)
    if not os.path.isfile(mv_f):
        print("cargo build failed!")
        sys.exit(2)
    shutil.copyfile(mv_f, os.path.join(r'{{DPM_INSTALL_PATH}}', bin_f))
    print("suc!")


[macos]
__move_to_server bin_file:
    scp ./{{bin_file}}/target/release/{{bin_file}} dwb@pinyin-ci.bbclient.icu:/Users/dwb/Desktop/ccmd-server/shizi_dev/{{bin_file}}/bin/Darwin
    scp ./scripts/install_{{bin_file}}.sh dwb@pinyin-ci.bbclient.icu:/Users/dwb/Desktop/ccmd-server/shizi_dev/{{bin_file}}/install.sh



public_dpm tar:
    #!{{PY_SHEBANG}}
    import os
    import sys
    import tempfile
    import shutil
    bin_f = sys.platform == "win32" and "{{tar}}.exe" or "{{tar}}"
    mv_f = os.path.join(r'{{justfile_directory()}}', r'{{tar}}', 'target', "release", bin_f)
    mv_config = os.path.join(r'{{justfile_directory()}}', r'{{tar}}', 'dpm.yml')

    t = tempfile.mktemp()
    os.mkdir(t)
    os.chdir(t)
    print(t, "-->>t")
    shutil.copyfile(mv_f, os.path.join(t, bin_f))
    shutil.copyfile(mv_config, os.path.join(t, "dpm.yml"))
    ret = os.system("cd %s && dpm publish"%t)
    if ret != 0: 
        sys.exit(1)


install_loc tar method="release" :
    just __build_cargo {{method}} {{tar}}
    just __mv_loc {{method}} {{tar}}

#下载指定的release
[windows]
__download_github_release release:
    python scripts/download_release.py {{release}} $env:GITHUB_TOKEN 

