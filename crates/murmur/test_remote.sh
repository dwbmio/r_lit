#!/bin/bash

# Murmur 跨机器协同编辑测试
#
# 使用方法：
# 1. 在远程机器上运行此脚本
# 2. 在本地机器上运行相同的程序
# 3. 使用相同的 group_id
# 4. 测试数据同步

echo "🚀 Murmur 跨机器协同编辑测试"
echo "============================"
echo ""

# 检查二进制文件是否存在
if [ ! -f "./manual_test" ]; then
    echo "❌ 错误：找不到 manual_test 可执行文件"
    echo "   请确保已上传 manual_test 到当前目录"
    exit 1
fi

# 确保可执行
chmod +x ./manual_test

echo "✅ 准备就绪"
echo ""
echo "测试信息："
echo "  - 本机器将作为远程节点"
echo "  - 请在本地机器上同时运行 manual_test"
echo "  - 使用相同的 group_id（建议: test_cross_machine）"
echo ""
echo "启动程序..."
echo ""

./manual_test
