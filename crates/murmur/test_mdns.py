#!/usr/bin/env python3
"""
最小化 mDNS 测试
用于验证 macOS 上的 mDNS 是否正常工作
"""

import socket
import struct
import time

# mDNS 多播地址和端口
MDNS_ADDR = '224.0.0.251'
MDNS_PORT = 5353

def send_mdns_query():
    """发送 mDNS 查询"""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)

    # 构造 mDNS 查询包（查询 _murmur._udp.local）
    query = b'\x00\x00'  # Transaction ID
    query += b'\x00\x00'  # Flags
    query += b'\x00\x01'  # Questions: 1
    query += b'\x00\x00'  # Answer RRs
    query += b'\x00\x00'  # Authority RRs
    query += b'\x00\x00'  # Additional RRs

    # Question: _murmur._udp.local
    query += b'\x07_murmur\x04_udp\x05local\x00'  # Name
    query += b'\x00\x0c'  # Type: PTR
    query += b'\x00\x01'  # Class: IN

    print(f"📡 发送 mDNS 查询到 {MDNS_ADDR}:{MDNS_PORT}")
    sock.sendto(query, (MDNS_ADDR, MDNS_PORT))
    sock.close()

def listen_mdns():
    """监听 mDNS 响应"""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.bind(('', MDNS_PORT))

    # 加入多播组
    mreq = struct.pack('4sl', socket.inet_aton(MDNS_ADDR), socket.INADDR_ANY)
    sock.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, mreq)

    sock.settimeout(5.0)

    print(f"👂 监听 mDNS 响应（5秒）...")
    print("   请在另一个终端运行 manual_test")
    print("")

    found_any = False
    try:
        while True:
            data, addr = sock.recvfrom(1024)
            if b'_murmur' in data or b'murmur' in data:
                print(f"✅ 收到 mDNS 包 from {addr}: {len(data)} bytes")
                found_any = True
    except socket.timeout:
        if not found_any:
            print("❌ 5秒内没有收到任何 _murmur 相关的 mDNS 包")
            print("")
            print("可能的原因：")
            print("  1. manual_test 没有运行")
            print("  2. mDNS 广播被阻止（防火墙/网络配置）")
            print("  3. mdns-sd 库没有正确广播")

    sock.close()

if __name__ == '__main__':
    print("🔍 mDNS 最小化测试")
    print("==================")
    print("")

    # 先监听
    listen_mdns()

    print("")
    print("测试完成！")
