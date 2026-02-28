#!/bin/bash

# 测试 group_vibe_workbench 协同编辑功能
#
# 使用方法：
# 1. 在终端1运行：./test_collaboration.sh user1
# 2. 在终端2运行：./test_collaboration.sh user2
# 3. 在两个窗口中都点击"搜索群组"
# 4. 在一个窗口中创建群组
# 5. 在另一个窗口中加入该群组
# 6. 测试协同编辑

set -e

USER_NAME=${1:-"测试用户"}

echo "🚀 启动 Group Vibe Workbench"
echo "================================"
echo "用户名: $USER_NAME"
echo ""
echo "提示："
echo "  1. 点击'搜索群组'按钮"
echo "  2. 如果是第一个用户，点击'创建新群组'"
echo "  3. 如果是第二个用户，等待看到群组后点击'加入'"
echo "  4. 进入群组大厅后，点击'开始协作'"
echo "  5. 编辑 ./chat.ctx 文件测试同步"
echo ""

cd "$(dirname "$0")"

# 清理旧数据（可选）
# rm -rf ./workbench_data

# 启动应用
./target/release/group_vibe_workbench launch --nickname "$USER_NAME"
