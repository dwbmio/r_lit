#!/bin/bash

# 测试 debug 模式下的多实例支持

echo "🧪 测试 Debug 模式多实例"
echo "======================="
echo ""

# 清理旧数据
rm -rf ./workbench_data_* 2>/dev/null || true

echo "✅ Debug 模式：允许多实例运行"
echo ""
echo "现在可以在两个终端中运行："
echo ""
echo "  终端 1: cargo run -- launch --nickname '用户1'"
echo "  终端 2: cargo run -- launch --nickname '用户2'"
echo ""
echo "预期结果："
echo "  ✅ 两个实例都能正常启动"
echo "  ✅ 使用不同的数据目录 (workbench_data_PID1 和 workbench_data_PID2)"
echo "  ✅ 能够通过 mDNS 发现彼此"
echo ""
