#!/bin/bash

# 跨机器测试 - 本地端脚本

echo "🚀 Murmur 跨机器协同编辑测试 - 本地端"
echo "========================================"
echo ""
echo "测试配置："
echo "  本地机器: macOS"
echo "  远程机器: admin@starlink-mars.hungrystudio.pp.ua"
echo "  测试目录: ~/data0/murmur-test"
echo ""

# 检查远程机器是否已准备好
echo "📡 检查远程机器连接..."
if ssh -o ConnectTimeout=5 admin@starlink-mars.hungrystudio.pp.ua 'echo ok' 2>/dev/null | grep -q ok; then
    echo "✅ 远程机器连接正常"
else
    echo "❌ 无法连接到远程机器"
    echo "   请检查网络连接和 SSH 配置"
    exit 1
fi

echo ""
echo "📦 上传测试文件..."
ssh admin@starlink-mars.hungrystudio.pp.ua 'mkdir -p ~/data0/murmur-test'
scp target/release/examples/manual_test test_remote.sh admin@starlink-mars.hungrystudio.pp.ua:~/data0/murmur-test/
ssh admin@starlink-mars.hungrystudio.pp.ua 'chmod +x ~/data0/murmur-test/manual_test ~/data0/murmur-test/test_remote.sh'

echo "✅ 文件上传完成"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "📋 测试步骤："
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "1️⃣  在远程机器上启动测试程序："
echo ""
echo "   ssh admin@starlink-mars.hungrystudio.pp.ua"
echo "   cd ~/data0/murmur-test"
echo "   ./test_remote.sh"
echo ""
echo "   输入昵称: RemoteUser"
echo "   输入群组: test_cross_machine"
echo ""
echo "2️⃣  在本地机器上启动测试程序："
echo ""
echo "   cargo run --release --example manual_test"
echo ""
echo "   输入昵称: LocalUser"
echo "   输入群组: test_cross_machine"
echo ""
echo "3️⃣  等待节点互相发现（约 5-10 秒）"
echo ""
echo "   应该看到: ✅ Connected to 1 peer(s)"
echo ""
echo "4️⃣  测试数据同步："
echo ""
echo "   本地写入:"
echo "   > write greeting Hello from local!"
echo ""
echo "   远程读取:"
echo "   > read greeting"
echo "   应该看到: ✅ Read: greeting = Hello from local!"
echo ""
echo "5️⃣  反向测试："
echo ""
echo "   远程写入:"
echo "   > write reply Hello from remote!"
echo ""
echo "   本地读取:"
echo "   > read reply"
echo "   应该看到: ✅ Read: reply = Hello from remote!"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
read -p "按 Enter 开始在本地启动测试程序..."
echo ""

cargo run --release --example manual_test
