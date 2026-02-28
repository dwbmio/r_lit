#!/bin/bash

# 快速局域网测试脚本

echo "🧪 Murmur 局域网协同编辑测试"
echo "============================="
echo ""
echo "此脚本将帮助你在同一台机器上测试两个实例"
echo ""

cd "$(dirname "$0")"

# 检查是否已编译
if [ ! -f "target/release/examples/manual_test" ]; then
    echo "📦 编译测试程序..."
    cargo build --release --example manual_test 2>&1 | tail -3
    echo ""
fi

echo "✅ 准备就绪"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📋 测试步骤"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "1️⃣  打开第二个终端窗口"
echo ""
echo "2️⃣  在第二个终端中运行："
echo "    cd $(pwd)"
echo "    cargo run --release --example manual_test"
echo ""
echo "    输入昵称: Bob"
echo "    输入群组: test_lan"
echo ""
echo "3️⃣  在本终端（第一个终端）中："
echo "    输入昵称: Alice"
echo "    输入群组: test_lan"
echo ""
echo "4️⃣  等待 5-10 秒，应该看到："
echo "    ✅ Connected to 1 peer(s)"
echo ""
echo "5️⃣  测试数据同步："
echo ""
echo "    Alice 终端:"
echo "    > write greeting Hello from Alice!"
echo ""
echo "    Bob 终端:"
echo "    > read greeting"
echo "    应该看到: ✅ Read: greeting = Hello from Alice!"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
read -p "按 Enter 在本终端启动第一个实例 (Alice)..."
echo ""

cargo run --release --example manual_test
