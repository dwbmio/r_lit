#!/bin/bash

# 测试 release 模式下的单实例限制

echo "🧪 测试 Release 模式单实例限制"
echo "=============================="
echo ""

# 编译 release 版本
echo "📦 编译 release 版本..."
cargo build --release 2>&1 | tail -3
echo ""

# 清理旧数据
rm -rf ./workbench_data 2>/dev/null || true

echo "✅ Release 模式：强制单实例"
echo ""
echo "测试步骤："
echo "  1. 启动第一个实例"
echo "  2. 尝试启动第二个实例"
echo ""
echo "预期结果："
echo "  ✅ 第一个实例正常启动"
echo "  ❌ 第二个实例被阻止，显示系统弹窗"
echo ""

read -p "按 Enter 启动第一个实例..."
./target/release/group_vibe_workbench launch --nickname "用户1" &
FIRST_PID=$!
echo "✅ 第一个实例已启动 (PID: $FIRST_PID)"
echo ""

sleep 3

read -p "按 Enter 尝试启动第二个实例（应该被阻止）..."
./target/release/group_vibe_workbench launch --nickname "用户2" 2>&1 | head -10
echo ""

echo "测试完成！"
echo ""
echo "清理："
read -p "按 Enter 停止第一个实例..."
kill $FIRST_PID 2>/dev/null || true
echo "✅ 已停止"
