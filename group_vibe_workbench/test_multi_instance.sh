#!/bin/bash

# 测试多进程协同编辑
#
# 这个脚本会启动两个 group_vibe_workbench 实例来测试协同编辑功能

echo "🧪 测试多进程协同编辑"
echo "===================="
echo ""
echo "准备启动两个实例..."
echo ""

# 清理旧的数据目录
echo "🧹 清理旧数据..."
rm -rf ./workbench_data_* 2>/dev/null || true

echo ""
echo "✅ 准备完成！"
echo ""
echo "请在两个终端中分别运行："
echo ""
echo "  终端 1: cargo run -- launch --nickname '用户1'"
echo "  终端 2: cargo run -- launch --nickname '用户2'"
echo ""
echo "测试步骤："
echo "  1. 在两个窗口中都点击 '搜索群组'"
echo "  2. 在终端1中点击 '创建新群组'"
echo "  3. 在终端2中应该能看到该群组，点击 '加入'"
echo "  4. 两个用户都进入群组大厅后，点击 '开始协作'"
echo "  5. 编辑 chat.ctx 文件，观察是否同步"
echo ""
echo "预期结果："
echo "  ✅ 两个进程都能正常启动（不会有数据库锁冲突）"
echo "  ✅ 能够通过 mDNS 发现彼此"
echo "  ✅ 能够建立 P2P 连接"
echo "  ✅ 文件修改能够实时同步"
echo ""
