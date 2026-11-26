#!/bin/bash
# distill 一键安装脚本（与 starlink-nebula / snappywatt 家族保持一致）
# 使用方式: bash -c "$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/install.sh)"
#
# 可覆盖的环境变量：
#   INSTALL_DIR       默认 $HOME/.snappywatt/bin（snappywatt 全家桶统一目录）
#   SNAPPYWATT_HOME   默认 $HOME/.snappywatt（影响 INSTALL_DIR 的父目录）
#   BINARY_NAME       下载的二进制名（默认 distill；服务端部署可传 BINARY_NAME=distilld）
#   INSTALL_NAME      安装后命令名（默认与 BINARY_NAME 同名）
#   VERSION           安装的版本目录（默认 latest；可锁定如 VERSION=v1.1.1）
#   S3_BASE/S3_BUCKET/S3_PATH   私有镜像地址覆盖

set -e

S3_BASE="${S3_BASE:-https://synology-minio.youxi123.com}"
S3_BUCKET="${S3_BUCKET:-starlink-artifacts}"
S3_PATH="${S3_PATH:-distill}"
BINARY_NAME="${BINARY_NAME:-distill}"
INSTALL_NAME="${INSTALL_NAME:-${BINARY_NAME}}"
VERSION="${VERSION:-latest}"

SNAPPYWATT_HOME="${SNAPPYWATT_HOME:-$HOME/.snappywatt}"
INSTALL_DIR="${INSTALL_DIR:-$SNAPPYWATT_HOME/bin}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1" >&2; exit 1; }

get_platform() {
    local os_name=$(uname -s)
    local arch_name=$(uname -m)
    case "$os_name" in
        Darwin)
            [[ "$arch_name" == "arm64" ]] && echo "aarch64-apple-darwin" || echo "x86_64-apple-darwin" ;;
        Linux)
            [[ "$arch_name" == "aarch64" || "$arch_name" == "arm64" ]] \
                && echo "aarch64-unknown-linux-gnu" || echo "x86_64-unknown-linux-gnu" ;;
        *) echo "unsupported" ;;
    esac
}

check_dependencies() {
    command -v curl  &>/dev/null || error "curl 未安装"
    command -v gunzip &>/dev/null || error "gunzip 未安装"
}

download_binary() {
    local platform="$1" output_path="$2"
    local url="${S3_BASE}/${S3_BUCKET}/${S3_PATH}/${VERSION}/${platform}/${BINARY_NAME}.gz"

    info "平台: ${platform}"
    info "下载: ${url}"

    local tmp_gz="${output_path}.gz"
    curl -fsSL -o "${tmp_gz}" "${url}?t=$(date +%s)" || error "下载失败，请检查网络或地址"

    info "解压..."
    gunzip -f "${tmp_gz}" || error "解压失败"
    chmod +x "${output_path}"
}

install_binary() {
    local src="$1"
    local dst="${INSTALL_DIR}/${INSTALL_NAME}"

    mkdir -p "${INSTALL_DIR}" || error "无法创建目录 ${INSTALL_DIR}"
    info "安装到: ${dst}"
    mv -f "$src" "$dst"
    chmod +x "$dst"

    local legacy="/usr/local/bin/${INSTALL_NAME}"
    if [[ -f "$legacy" && -w "/usr/local/bin" ]]; then
        rm -f "$legacy" && warn "已移除旧版本: $legacy"
    elif [[ -f "$legacy" ]]; then
        warn "检测到旧版 ${legacy}（无写权限，未删除）。如需手动清理: sudo rm ${legacy}"
    fi
}

ensure_in_path() {
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) info "PATH 已包含 ${INSTALL_DIR}"; return 0 ;;
    esac

    local rc=""
    local line="export PATH=\"${INSTALL_DIR}:\$PATH\""
    local marker="# Added by snappywatt installer (${INSTALL_NAME})"

    case "${SHELL##*/}" in
        zsh)
            rc="${ZDOTDIR:-$HOME}/.zshrc" ;;
        bash)
            if [[ -f "$HOME/.bashrc" ]]; then rc="$HOME/.bashrc"; else rc="$HOME/.bash_profile"; fi ;;
        fish)
            rc="$HOME/.config/fish/config.fish"
            line="fish_add_path -p ${INSTALL_DIR}" ;;
        *)
            warn "未识别的 shell（${SHELL}），请手动把 ${INSTALL_DIR} 加进 PATH"
            return ;;
    esac

    mkdir -p "$(dirname "$rc")"
    if [[ -f "$rc" ]] && grep -qsF "snappywatt installer" "$rc"; then
        info "PATH 已配置过（${rc}）"
    else
        printf '\n%s\n%s\n' "$marker" "$line" >> "$rc"
        info "已写入 PATH 到 ${rc}"
    fi

    warn "请执行: source ${rc}    或重开终端，让 ${INSTALL_NAME} 在新会话中生效"
}

main() {
    echo ""
    echo "=========================================="
    echo "    ${INSTALL_NAME} 一键安装"
    echo "=========================================="
    echo ""

    check_dependencies

    local platform=$(get_platform)
    [[ "$platform" == "unsupported" ]] && error "不支持的平台: $(uname -s) $(uname -m)"

    local tmp_file=$(mktemp)
    download_binary "$platform" "$tmp_file"
    install_binary "$tmp_file"
    rm -f "$tmp_file" "$tmp_file.gz" 2>/dev/null || true

    ensure_in_path

    echo ""
    if "${INSTALL_DIR}/${INSTALL_NAME}" --version &>/dev/null; then
        info "安装成功: $("${INSTALL_DIR}/${INSTALL_NAME}" --version)"
    else
        warn "安装到 ${INSTALL_DIR}/${INSTALL_NAME}，但未能直接运行（可手动验证）"
    fi
    info "安装目录: ${INSTALL_DIR}"
    info "如需自定义路径: INSTALL_DIR=/your/path bash -c \"\$(curl -fsSL ${S3_BASE}/${S3_BUCKET}/${S3_PATH}/install.sh)\""
}

main "$@"
