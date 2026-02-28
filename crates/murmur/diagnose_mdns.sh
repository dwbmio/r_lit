#!/bin/bash

# mDNS 诊断脚本

echo "🔍 mDNS 诊断工具"
echo "================"
echo ""

echo "1️⃣  检查 mDNS 服务是否运行..."
if pgrep -x "mDNSResponder" > /dev/null; then
    echo "✅ mDNSResponder 正在运行"
else
    echo "❌ mDNSResponder 未运行"
fi
echo ""

echo "2️⃣  监听 _murmur._udp 服务（10秒）..."
echo "   请在另一个终端启动 manual_test"
echo ""

timeout 10 dns-sd -B _murmur._udp local. 2>&1 || {
    echo ""
    echo "⚠️  没有发现 _murmur._udp 服务"
    echo ""
    echo "可能的原因："
    echo "  1. manual_test 没有运行"
    echo "  2. mDNS 广播被阻止"
    echo "  3. 网络接口问题"
}

echo ""
echo "3️⃣  检查网络接口..."
ifconfig | grep -E "^[a-z]|inet " | head -20

echo ""
echo "4️⃣  检查防火墙状态..."
sudo /usr/libexec/ApplicationFirewall/socketfilterfw --getglobalstate

echo ""
echo "诊断完成！"
