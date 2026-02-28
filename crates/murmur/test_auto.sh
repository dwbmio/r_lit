#!/bin/bash

# 自动化测试 murmur 双进程协同编辑

set -e

echo "🧪 测试 Murmur 双进程协同编辑"
echo "============================="
echo ""

cd "$(dirname "$0")"

# 清理旧数据
rm -rf /tmp/murmur_auto_test_* 2>/dev/null || true

echo "📦 编译 murmur..."
cargo build --example manual_test 2>&1 | tail -3
echo ""

# 创建测试输入文件
cat > /tmp/test_input_1.txt <<EOF
Alice
test_group_auto
write greeting Hello from Alice!
read greeting
quit
EOF

cat > /tmp/test_input_2.txt <<EOF
Bob
test_group_auto
write greeting Hello from Bob!
read greeting
quit
EOF

echo "🚀 启动第一个实例 (Alice)..."
timeout 30 ./target/debug/examples/manual_test < /tmp/test_input_1.txt > /tmp/test_output_1.txt 2>&1 &
PID1=$!
echo "   PID: $PID1"

sleep 5

echo "🚀 启动第二个实例 (Bob)..."
timeout 30 ./target/debug/examples/manual_test < /tmp/test_input_2.txt > /tmp/test_output_2.txt 2>&1 &
PID2=$!
echo "   PID: $PID2"

echo ""
echo "⏳ 等待测试完成 (最多 35 秒)..."
sleep 35

echo ""
echo "📊 测试结果："
echo ""
echo "=== Alice 的输出 ==="
cat /tmp/test_output_1.txt | grep -E "(Connected to|Wrote:|Read:)" || echo "无相关输出"
echo ""
echo "=== Bob 的输出 ==="
cat /tmp/test_output_2.txt | grep -E "(Connected to|Wrote:|Read:)" || echo "无相关输出"
echo ""

# 检查是否成功连接
if grep -q "Connected to 1 peer" /tmp/test_output_1.txt && grep -q "Connected to 1 peer" /tmp/test_output_2.txt; then
    echo "✅ 成功：两个节点互相发现并连接"
else
    echo "❌ 失败：节点未能互相发现"
    echo ""
    echo "完整日志："
    echo "=== Alice 完整输出 ==="
    cat /tmp/test_output_1.txt
    echo ""
    echo "=== Bob 完整输出 ==="
    cat /tmp/test_output_2.txt
fi

# 清理
kill $PID1 $PID2 2>/dev/null || true
rm -f /tmp/test_input_*.txt /tmp/test_output_*.txt
rm -rf /tmp/murmur_auto_test_*

echo ""
echo "测试完成！"
