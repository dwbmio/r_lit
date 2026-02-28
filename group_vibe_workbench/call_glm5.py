#!/usr/bin/env python3
"""
调用 GLM5 API 来寻求 WebView 实现帮助
"""

import json
import requests

# GLM5 API 配置
API_KEY = "79f15f08c188470eacef152178af2234.fJQDjiorFbACdDNc"
API_URL = "https://open.bigmodel.cn/api/paas/v4/chat/completions"

def read_context():
    """读取上下文文件"""
    with open("GLM5_REQUEST.md", "r", encoding="utf-8") as f:
        return f.read()

def call_glm5(prompt):
    """调用 GLM5 API"""
    headers = {
        "Authorization": f"Bearer {API_KEY}",
        "Content-Type": "application/json"
    }

    data = {
        "model": "glm-4-plus",  # 或 "glm-4" 根据你的订阅
        "messages": [
            {
                "role": "system",
                "content": "你是一个 Rust 和 GPUI 框架的专家，擅长解决复杂的系统编程问题。"
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "temperature": 0.7,
        "max_tokens": 4000
    }

    try:
        response = requests.post(API_URL, headers=headers, json=data, timeout=120)
        response.raise_for_status()
        result = response.json()
        return result["choices"][0]["message"]["content"]
    except Exception as e:
        return f"错误: {str(e)}\n响应: {response.text if 'response' in locals() else 'N/A'}"

def main():
    print("正在读取上下文...")
    context = read_context()

    print(f"上下文长度: {len(context)} 字符")
    print("\n正在调用 GLM5 API...")
    print("=" * 80)

    response = call_glm5(context)

    print("\nGLM5 响应:")
    print("=" * 80)
    print(response)
    print("=" * 80)

    # 保存响应
    with open("GLM5_RESPONSE.md", "w", encoding="utf-8") as f:
        f.write("# GLM5 响应 - WebView 实现方案\n\n")
        f.write(response)

    print("\n响应已保存到 GLM5_RESPONSE.md")

if __name__ == "__main__":
    main()
