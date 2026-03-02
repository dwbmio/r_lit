#!/bin/bash
# 快速启动测试脚本

echo "🚀 Murmur 协作测试 - 快速启动"
echo ""
echo "请选择你的角色："
echo "  1) Alice (第一个节点)"
echo "  2) Bob (第二个节点)"
echo "  3) Charlie (第三个节点)"
echo ""
read -p "选择 (1/2/3): " choice

case $choice in
  1)
    echo ""
    echo "启动 Alice..."
    echo "Alice" | cargo run --release --example manual_test
    ;;
  2)
    echo ""
    echo "启动 Bob..."
    echo "Bob" | cargo run --release --example manual_test
    ;;
  3)
    echo ""
    echo "启动 Charlie..."
    echo "Charlie" | cargo run --release --example manual_test
    ;;
  *)
    echo "无效选择"
    exit 1
    ;;
esac
