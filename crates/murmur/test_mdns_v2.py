#!/usr/bin/env python3
"""
简化的 mDNS 监听器
使用 tcpdump 抓包来检测 mDNS 流量
"""

import subprocess
import sys
import time

def check_mdns_traffic():
    """使用 tcpdump 监听 mDNS 流量"""
    print("🔍 mDNS 流量监听")
    print("================")
    print("")
    print("⚠️  需要 sudo 权限来运行 tcpdump")
    print("")
    print("👂 监听 10 秒，请在另一个终端启动 manual_test...")
    print("")

    try:
        # 使用 tcpdump 监听 mDNS 流量
        cmd = [
            'sudo', 'tcpdump',
            '-i', 'any',           # 所有接口
            '-n',                  # 不解析主机名
            'udp', 'port', '5353', # mDNS 端口
            '-c', '20',            # 最多抓 20 个包
            '-v'                   # 详细输出
        ]

        print("执行命令:", ' '.join(cmd))
        print("")

        result = subprocess.run(
            cmd,
            timeout=10,
            capture_output=True,
            text=True
        )

        output = result.stdout + result.stderr

        print("抓包结果:")
        print("=" * 60)
        print(output)
        print("=" * 60)
        print("")

        # 分析结果
        if '_murmur' in output.lower() or 'murmur' in output.lower():
            print("✅ 检测到 _murmur 相关的 mDNS 流量！")
            print("   mDNS 广播正常工作")
        elif 'udp' in output.lower() and '5353' in output:
            print("⚠️  检测到 mDNS 流量，但没有 _murmur 服务")
            print("   可能原因：")
            print("   1. manual_test 没有运行")
            print("   2. mdns-sd 库没有正确广播")
        else:
            print("❌ 没有检测到任何 mDNS 流量")
            print("   可能原因：")
            print("   1. mDNS 被防火墙阻止")
            print("   2. 网络接口问题")

    except subprocess.TimeoutExpired:
        print("⏱️  10 秒超时")
    except PermissionError:
        print("❌ 权限不足，请使用 sudo 运行此脚本")
    except FileNotFoundError:
        print("❌ 找不到 tcpdump 命令")
        print("   请安装: brew install tcpdump")
    except Exception as e:
        print(f"❌ 错误: {e}")

def simple_check():
    """简单检查：使用 dns-sd 命令"""
    print("🔍 使用 dns-sd 检查 mDNS 服务")
    print("================================")
    print("")
    print("👂 监听 5 秒，请确保 manual_test 正在运行...")
    print("")

    try:
        cmd = ['dns-sd', '-B', '_murmur._udp', 'local.']

        print("执行命令:", ' '.join(cmd))
        print("")

        proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True
        )

        # 等待 5 秒
        time.sleep(5)
        proc.terminate()

        stdout, stderr = proc.communicate(timeout=2)

        print("输出:")
        print("=" * 60)
        print(stdout)
        if stderr:
            print("错误:", stderr)
        print("=" * 60)
        print("")

        if 'murmur' in stdout.lower():
            print("✅ 发现 _murmur._udp 服务！")
            print("   mDNS 正常工作")
            return True
        else:
            print("❌ 没有发现 _murmur._udp 服务")
            print("   可能 manual_test 没有运行，或 mdns-sd 库有问题")
            return False

    except Exception as e:
        print(f"❌ 错误: {e}")
        return False

if __name__ == '__main__':
    print("=" * 60)
    print("mDNS 诊断工具")
    print("=" * 60)
    print("")

    # 先用简单方法
    print("方法 1: 使用 dns-sd 命令")
    print("-" * 60)
    found = simple_check()

    print("")
    print("")

    if not found:
        print("方法 2: 使用 tcpdump 抓包（需要 sudo）")
        print("-" * 60)
        response = input("是否运行 tcpdump？(y/n): ")
        if response.lower() == 'y':
            check_mdns_traffic()

    print("")
    print("诊断完成！")
