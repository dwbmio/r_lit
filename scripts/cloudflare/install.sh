#!/bin/bash

abort() {
    printf "%s\n" "$@" >&2
    exit 1
}

# 下载函数
download_bin() {
    echo "downloading..."
    echo "匹配下载地址: $1"
    
    # 定义键和值数组
    keys=("Darwin_arm64" "Darwin_x86_64" "Linux")
    download_urls=(
        "https://dwb.ren/dry/dpm/dpm-aarch64-apple-darwin" 
        "https://dwb.ren/dry/dpm/dpm-x86_64-apple-darwin" 
        "https://dwb.ren/dry/dpm/dpm-x86_64-unknown-linux-musl")
    
    # 读取传入的 key
    key="$1"
    
    # 遍历 keys 数组，查找匹配的 key
    found=0
    for i in "${!keys[@]}"; do
        if [[ "${keys[$i]}" == "$key" ]]; then
            url="${download_urls[$i]}"
            echo "Downloading content from: $url"
            
            # 下载文件到当前目录，命名为 "dpm"
            curl -o "dpm" "$url?nocache=$(date +%s)"
            
            echo "下载完成！"
            found=1
            break
        fi
    done
    
    # 如果未找到匹配的 key，则提示不支持
    if [[ $found -eq 0 ]]; then
        echo "当前系统不支持: $key"
    fi
}

# 获取系统信息（用以自动匹配平台）
get_system_key() {
    os_name=$(uname -s)       # 获取操作系统名称
    arch_name=$(uname -m)     # 获取架构名称
    
    case "$os_name" in
        Darwin)
            if [[ "$arch_name" == "arm64" ]]; then
                echo "Darwin_arm64"
            else
                echo "Darwin_x86_64"
            fi
            ;;
        Linux)
            echo "Linux"
            ;;
        *)
            echo "Unsupported"
            ;;
    esac
}

install_bin() {
    echo "move to bin..."
    sudo mv -f dpm /usr/local/bin
        sudo chmod a+x /usr/local/bin/dpm
    echo "install suc!"
    echo "Just run dpm in command tool to have test."
}

# ==========process 

# 主程序逻辑
key=$(get_system_key)  # 自动确定当前系统的 key
if [[ "$key" == "Unsupported" ]]; then
    echo "当前系统不支持此下载程序。"
    exit 1
fi
echo "Current os is ${key}..."
download_bin "$key"
install_bin