#!/bin/sh
# 安装脚本 - 自动检测平台并下载对应的二进制文件

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置
REPO="YOUR_GITHUB_USERNAME/r_lit"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# 检测操作系统和架构
detect_platform() {
    local os=$(uname -s | tr '[:upper:]' '[:lower:]')
    local arch=$(uname -m)

    case "$os" in
        linux*)
            OS="linux"
            ;;
        darwin*)
            OS="darwin"
            ;;
        mingw* | msys* | cygwin*)
            OS="windows"
            ;;
        *)
            echo "${RED}不支持的操作系统: $os${NC}"
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64 | amd64)
            ARCH="x86_64"
            ;;
        aarch64 | arm64)
            ARCH="aarch64"
            ;;
        *)
            echo "${RED}不支持的架构: $arch${NC}"
            exit 1
            ;;
    esac

    # 构建目标三元组
    case "$OS" in
        linux)
            TARGET="${ARCH}-unknown-linux-gnu"
            EXT="tar.gz"
            ;;
        darwin)
            TARGET="${ARCH}-apple-darwin"
            EXT="tar.gz"
            ;;
        windows)
            TARGET="${ARCH}-pc-windows-gnu"
            EXT="zip"
            ;;
    esac

    echo "${GREEN}检测到平台: $OS $ARCH${NC}"
    echo "${GREEN}目标: $TARGET${NC}"
}

# 获取最新版本
get_latest_version() {
    echo "${YELLOW}获取最新版本...${NC}"
    VERSION=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$VERSION" ]; then
        echo "${RED}无法获取最新版本${NC}"
        exit 1
    fi

    echo "${GREEN}最新版本: $VERSION${NC}"
}

# 下载并安装工具
install_tool() {
    local tool=$1
    local filename="${tool}-${TARGET}.${EXT}"
    local url="https://github.com/$REPO/releases/download/$VERSION/$filename"

    echo "${YELLOW}下载 $tool...${NC}"
    echo "URL: $url"

    # 创建临时目录
    local tmp_dir=$(mktemp -d)
    cd "$tmp_dir"

    # 下载文件
    if ! curl -L -o "$filename" "$url"; then
        echo "${RED}下载失败: $tool${NC}"
        rm -rf "$tmp_dir"
        return 1
    fi

    # 解压
    echo "${YELLOW}解压 $tool...${NC}"
    if [ "$EXT" = "tar.gz" ]; then
        tar xzf "$filename"
    else
        unzip -q "$filename"
    fi

    # 安装
    echo "${YELLOW}安装 $tool 到 $INSTALL_DIR...${NC}"
    mkdir -p "$INSTALL_DIR"

    if [ "$OS" = "windows" ]; then
        mv "${tool}.exe" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/${tool}.exe"
    else
        mv "$tool" "$INSTALL_DIR/"
        chmod +x "$INSTALL_DIR/$tool"
    fi

    # 清理
    cd - > /dev/null
    rm -rf "$tmp_dir"

    echo "${GREEN}✓ $tool 安装成功${NC}"
}

# 验证安装
verify_installation() {
    local tool=$1

    if [ "$OS" = "windows" ]; then
        tool="${tool}.exe"
    fi

    if [ -x "$INSTALL_DIR/$tool" ]; then
        echo "${GREEN}✓ $tool 已安装到 $INSTALL_DIR/$tool${NC}"
        "$INSTALL_DIR/$tool" --version || true
    else
        echo "${RED}✗ $tool 安装失败${NC}"
        return 1
    fi
}

# 检查 PATH
check_path() {
    if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
        echo ""
        echo "${YELLOW}警告: $INSTALL_DIR 不在 PATH 中${NC}"
        echo "请将以下内容添加到你的 shell 配置文件 (~/.bashrc, ~/.zshrc 等):"
        echo ""
        echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
        echo ""
    fi
}

# 主函数
main() {
    echo "${GREEN}=== R_LIT 工具安装脚本 ===${NC}"
    echo ""

    # 检测平台
    detect_platform

    # 获取最新版本
    get_latest_version

    echo ""
    echo "${YELLOW}将安装以下工具:${NC}"
    echo "  - bulk_upload"
    echo "  - img_resize"
    echo ""
    echo "${YELLOW}安装目录: $INSTALL_DIR${NC}"
    echo ""

    # 确认安装
    if [ -t 0 ]; then
        printf "继续安装? [Y/n] "
        read -r response
        case "$response" in
            [nN][oO]|[nN])
                echo "安装已取消"
                exit 0
                ;;
        esac
    fi

    echo ""

    # 安装工具
    install_tool "bulk_upload"
    install_tool "img_resize"

    echo ""
    echo "${GREEN}=== 安装完成 ===${NC}"
    echo ""

    # 验证安装
    verify_installation "bulk_upload"
    verify_installation "img_resize"

    # 检查 PATH
    check_path

    echo ""
    echo "${GREEN}使用方法:${NC}"
    echo "  bulk_upload --help"
    echo "  img_resize --help"
    echo ""
}

# 运行主函数
main "$@"
