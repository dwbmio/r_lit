# WebView PopView 实现请求 - 给 GLM5

## 项目背景

我们正在开发一个基于 Rust + GPUI 0.2.2 的桌面协作应用 `group_vibe_workbench`。

### 技术栈
- **UI 框架**: GPUI 0.2.2 (GPU 加速的 Rust UI 框架)
- **组件库**: gpui-component 0.5.1
- **WebView**: wry (通过 gpui-component)
- **平台**: macOS, Linux, Windows

## 当前状态

### 已完成
✅ 纯 GPUI 版本的 LoadingPopView（使用 div + 样式）
✅ 基本的 UI 架构（Page/View/PopView/Toast）
✅ 群组发现、创建、加入功能

### 需要实现
❌ WebView 版本的 LoadingPopView，支持 HTML/CSS/JS 动画
❌ 未来可能集成 Rive 或 Lottie 动画

## 核心问题

### gpui-component 的 WebView API

```rust
// 来自 gpui-component-0.5.1/src/webview.rs
pub struct WebView {
    focus_handle: FocusHandle,
    webview: Rc<wry::WebView>,
    visible: bool,
    bounds: Bounds<Pixels>,
}

impl WebView {
    /// 需要手动创建 wry::WebView
    pub fn new(webview: wry::WebView, _: &mut Window, cx: &mut App) -> Self {
        // ...
    }
}
```

**问题**:
1. 需要手动创建 `wry::WebView` 实例
2. wry 需要原生窗口句柄（NSWindow on macOS, HWND on Windows）
3. GPUI 0.2.2 的 Window 结构不暴露原生句柄

### 我们尝试的代码

```rust
use gpui_component::webview::WebView;

pub struct LoadingWebViewPopView {
    message: String,
}

impl LoadingWebViewPopView {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn html_content(&self) -> String {
        format!(
            r#"
<!DOCTYPE html>
<html>
<head>
    <style>
        body {{
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            background: transparent;
        }}
        .spinner {{
            width: 80px;
            height: 80px;
            border: 8px solid rgba(137, 180, 250, 0.2);
            border-top-color: #89b4fa;
            border-radius: 50%;
            animation: spin 1s linear infinite;
        }}
        @keyframes spin {{
            to {{ transform: rotate(360deg); }}
        }}
    </style>
</head>
<body>
    <div class="spinner"></div>
    <p>{}</p>
</body>
</html>
            "#,
            self.message
        )
    }
}

// 问题：如何创建 wry::WebView？
// 问题：如何获取 GPUI Window 的原生句柄？
// 问题：如何在 PopView 中使用 WebView？
```

## 需要解决的问题

### 1. 如何创建 wry::WebView？

```rust
// wry 的标准用法
use wry::WebViewBuilder;

let webview = WebViewBuilder::new()
    .with_html("<html>...</html>")
    .build(&window_handle)?;  // 需要原生窗口句柄
```

**问题**: 如何从 GPUI 0.2.2 的 `Window` 获取原生窗口句柄？

### 2. GPUI Window 结构

```rust
// GPUI 0.2.2 的 Window 结构（简化）
pub struct Window {
    // 内部字段不公开
}

// 没有提供获取原生句柄的公开方法
```

**需要**: 找到获取原生窗口句柄的方法（可能需要 unsafe 代码）

### 3. 在 PopView 中集成 WebView

```rust
pub trait PopView {
    fn id(&self) -> &'static str;
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
}

// 如何在 render 方法中创建和使用 WebView？
```

## 相关源码位置

### GPUI 0.2.2
- 位置: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.2/`
- 平台层: `src/platform/mac/`, `src/platform/linux/`, `src/platform/windows/`
- Window: `src/window.rs`

### gpui-component 0.5.1
- 位置: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-component-0.5.1/`
- WebView: `src/webview.rs`

### 我们的代码
- 项目: `/Users/admin/data0/private_work/r_lit/group_vibe_workbench/`
- PopView trait: `src/gui/popview.rs`
- 尝试的实现: `src/gui/popviews/loading_webview.rs` (已删除)

## 期望的解决方案

### 理想情况

```rust
pub struct LoadingWebViewPopView {
    message: String,
    webview: Option<Entity<WebView>>,  // GPUI Entity
}

impl PopView for LoadingWebViewPopView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 1. 获取原生窗口句柄
        let native_handle = /* 如何获取？ */;

        // 2. 创建 wry::WebView
        let wry_webview = WebViewBuilder::new()
            .with_html(self.html_content())
            .build(&native_handle)?;

        // 3. 包装成 gpui-component 的 WebView
        let webview = WebView::new(wry_webview, window, cx);

        // 4. 渲染
        div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .child(webview)
    }
}
```

### 关键问题

1. **如何获取原生窗口句柄？**
   - macOS: NSWindow
   - Linux: X11 Window 或 Wayland Surface
   - Windows: HWND

2. **生命周期管理**
   - WebView 何时创建？
   - 如何避免重复创建？
   - 如何正确清理？

3. **跨平台兼容性**
   - 不同平台的窗口句柄类型不同
   - 如何编写跨平台代码？

## 可能的方向

### 方向 1: 深入 GPUI 内部

研究 GPUI 0.2.2 的平台层实现，找到访问原生窗口句柄的方法。

**可能需要**:
- 使用 unsafe 代码
- 访问私有字段
- 平台特定的代码

### 方向 2: 使用 GPUI 的扩展点

检查 GPUI 是否提供了扩展机制或钩子来访问底层窗口。

### 方向 3: 参考其他项目

查看是否有其他项目成功在 GPUI 0.2.2 中集成了 WebView。

## 请求

请帮助我们：

1. **分析 GPUI 0.2.2 源码**，找到获取原生窗口句柄的方法
2. **提供完整的实现代码**，包括：
   - 获取窗口句柄的代码（跨平台）
   - 创建 wry::WebView 的代码
   - 集成到 PopView 的代码
   - 生命周期管理
3. **处理边界情况**：
   - 错误处理
   - 跨平台兼容性
   - 性能优化

## 约束条件

- 必须使用 GPUI 0.2.2（不能升级）
- 必须使用 gpui-component 0.5.1
- 需要跨平台支持（macOS, Linux, Windows）
- 代码应该是安全的（尽量避免 unsafe，如果必须使用则要有充分的注释）

## 参考资料

### GPUI 0.2.2 Window 相关代码

```rust
// 来自 gpui-0.2.2/src/window.rs (部分)
pub struct Window {
    // 私有字段
}

impl Window {
    // 公开方法不包含获取原生句柄的功能
}
```

### wry WebView 创建示例

```rust
use wry::{WebViewBuilder, WebView};

#[cfg(target_os = "macos")]
fn create_webview(ns_window: *mut objc::runtime::Object) -> Result<WebView> {
    use wry::dpi::LogicalSize;

    let webview = WebViewBuilder::new()
        .with_html("<html><body>Hello</body></html>")
        .build_as_child(&ns_window)?;

    Ok(webview)
}
```

## 期望的输出

请提供：

1. **详细的实现步骤**
2. **完整的代码示例**（可以直接使用的）
3. **解释和注释**（说明为什么这样做）
4. **潜在的问题和解决方案**

谢谢！
