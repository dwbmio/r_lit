#!/bin/bash
# 快速测试 Dev Mode

echo "清理旧数据..."
rm -rf workbench_data_dev_*

echo "启动 Dev Mode (2 个实例)..."
./target/release/group_vibe_workbench dev -c 2

echo "测试完成"
