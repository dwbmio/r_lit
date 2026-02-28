# GUI 架构设计

Group Vibe Workbench 的用户界面采用分层组件架构，所有样式统一由主题系统管理。

## 组件颗粒度层级

### 1. Page - 全屏页面，独立路由

Page 是应用的顶层容器，每个 Page 对应一个独立的路由。

**特性：**
- 独立路由，可通过 Router 切换
- 全屏显示，同一时间只显示一个
- 生命周期钩子：`on_enter()`, `on_leave()`, `can_leave()`

**示例：**
```rust
struct HomePage;

impl Page for HomePage {
    fn id(&self) -> &'static str {
        "home"
    }

    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized
    {
        div().child("Home Page Content")
    }
}
```

### 2. View - Page 上的全屏区域

View 是 Page 内部的大块内容区域，通常占据整个 Page 或 Page 的主要部分。

**特性：**
- 属于 Page 内部，不影响路由
- 可在同一个 Page 内切换（如 Tab 切换）
- 生命周期钩子：`on_activate()`, `on_deactivate()`

**示例：**
```rust
struct EditorView {
    content: String,
}

impl View for EditorView {
    fn id(&self) -> &'static str {
        "editor"
    }

    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized
    {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(&self.content)
    }
}
```

### 3. PopView - 浮层弹窗，互斥显示

PopView 是浮动在页面之上的弹窗组件，同一时间只能显示一个。

**特性：**
- 互斥显示：新 PopView 打开时自动关闭当前的
- 遮罩层：可选的半透明背景
- 居中显示：默认在屏幕中央
- 可关闭：支持点击遮罩或 ESC 键关闭

**示例：**
```rust
struct SettingsPopView {
    settings: Settings,
}

impl PopView for SettingsPopView {
    fn id(&self) -> &'static str {
        "settings"
    }

    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized
    {
        div()
            .w(px(600.0))
            .h(px(400.0))
            .bg(rgb(theme.colors.surface))
            .child("Settings Content")
    }
}
```

### 4. Toast - 队列消息提示

Toast 是轻量级的消息提示组件，以队列形式显示。

**特性：**
- 队列显示：多个 Toast 按顺序排列
- 自动消失：可设置自动消失时间
- 分级显示：Info/Success/Warning/Error 不同样式
- 可关闭：支持手动关闭

**示例：**
```rust
// 显示成功提示
toast_queue.push(Toast::success("保存成功"));

// 显示错误提示，5秒后自动消失
toast_queue.push(Toast::error("网络连接失败").duration(5000));

// 显示信息提示，不自动消失
toast_queue.push(Toast::info("正在同步...").persistent());
```

## 主题系统

所有组件的样式都从统一的主题系统获取，确保视觉一致性。

### Theme 结构

```rust
pub struct Theme {
    pub name: String,
    pub colors: ColorScheme,      // 颜色方案
    pub typography: Typography,   // 字体系统
    pub spacing: Spacing,         // 间距系统
    pub radius: Radius,           // 圆角系统
    pub shadow: Shadow,           // 阴影系统
    pub animation: Animation,     // 动画系统
}
```

### 颜色方案

```rust
pub struct ColorScheme {
    // 基础颜色
    pub background: u32,          // 主背景色
    pub surface: u32,             // 次级背景色
    pub surface_variant: u32,     // 三级背景色

    // 文本颜色
    pub text: u32,                // 主文本颜色
    pub text_secondary: u32,      // 次级文本颜色
    pub text_disabled: u32,       // 禁用文本颜色

    // 边框颜色
    pub border: u32,              // 主边框颜色
    pub divider: u32,             // 分割线颜色

    // 语义颜色
    pub primary: u32,             // 主色调（品牌色）
    pub primary_hover: u32,       // 主色调悬停态
    pub success: u32,             // 成功色
    pub warning: u32,             // 警告色
    pub error: u32,               // 错误色
    pub info: u32,                // 信息色

    // 遮罩颜色
    pub mask: u32,                // 遮罩层背景色
    pub mask_opacity: f32,        // 遮罩层透明度
}
```

### 字体系统

```rust
pub struct Typography {
    pub font_family: String,      // 字体族
    pub font_family_mono: String, // 等宽字体族

    pub heading: TextStyle,       // 标题样式
    pub subheading: TextStyle,    // 副标题样式
    pub body: TextStyle,          // 正文样式
    pub caption: TextStyle,       // 小字样式
    pub button: TextStyle,        // 按钮文字样式
}

pub struct TextStyle {
    pub size: Pixels,             // 字体大小
    pub line_height: f32,         // 行高
    pub weight: u16,              // 字重
}
```

### 间距系统

基于 8px 基准的间距系统：

```rust
pub struct Spacing {
    pub xs: Pixels,   // 4px
    pub sm: Pixels,   // 8px
    pub md: Pixels,   // 16px
    pub lg: Pixels,   // 24px
    pub xl: Pixels,   // 32px
    pub xxl: Pixels,  // 48px
}
```

### 使用主题

```rust
// 获取当前主题
let theme = theme_manager.current();

// 使用主题颜色
div().bg(rgb(theme.colors.background));

// 使用主题字体
div().text_size(theme.typography.body.size);

// 使用主题间距
div().p(theme.spacing.md);

// 使用主题圆角
div().rounded(theme.radius.md);
```

### 切换主题

```rust
// 切换到浅色主题
theme_manager.switch("Catppuccin Latte");

// 切换到深色主题
theme_manager.switch("Catppuccin Mocha");

// 添加自定义主题
let custom_theme = Theme::custom("My Theme", custom_colors);
theme_manager.add_theme(custom_theme);
```

## 内置主题

### Catppuccin Mocha（默认深色主题）

- 背景色：`#1e1e2e`
- 表面色：`#313244`
- 主文本：`#cdd6f4`
- 主色调：`#89b4fa`（蓝色）
- 成功色：`#a6e3a1`（绿色）
- 警告色：`#f9e2af`（黄色）
- 错误色：`#f38ba8`（红色）

### Catppuccin Latte（浅色主题）

- 背景色：`#eff1f5`
- 表面色：`#e6e9ef`
- 主文本：`#4c4f69`
- 主色调：`#1e66f5`（蓝色）
- 成功色：`#40a02b`（绿色）
- 警告色：`#df8e1d`（黄色）
- 错误色：`#d20f39`（红色）

## 路由系统

Router 管理 Page 级别的路由切换：

```rust
// 创建路由器
let mut router = Router::new();

// 注册页面
router.register("home", Box::new(HomePage::new()));
router.register("editor", Box::new(EditorPage::new()));

// 导航到编辑器页面
router.navigate("editor");

// 返回上一页
router.back();

// 获取当前页面
let current = router.current_page();

// 检查是否可以返回
if router.can_back() {
    router.back();
}
```

## 设计原则

### 1. 单一数据源

所有样式定义都来自主题系统，组件不应硬编码样式值。

**✅ 正确：**
```rust
div().bg(rgb(theme.colors.background))
```

**❌ 错误：**
```rust
div().bg(rgb(0x1e1e2e))
```

### 2. 语义化命名

使用语义化的颜色名称，而非具体颜色值。

**✅ 正确：**
```rust
theme.colors.primary
theme.colors.success
```

**❌ 错误：**
```rust
theme.colors.blue
theme.colors.green
```

### 3. 组件分层

严格遵守组件层级关系：

```
Page (路由级别)
  └─ View (内容区域)
      ├─ PopView (浮层弹窗)
      └─ Toast (消息提示)
```

### 4. 主题优先

切换样式只通过切换主题实现，不应在组件内部处理样式变体。

## 文件结构

```
src/gui/
├── mod.rs          # 模块导出
├── theme.rs        # 主题系统
├── page.rs         # Page trait 和容器
├── view.rs         # View trait 和容器
├── popview.rs      # PopView trait 和管理器
├── toast.rs        # Toast 和队列管理器
└── router.rs       # 路由管理器
```

## 下一步

1. 创建具体的 Page 实现（HomePage, EditorPage 等）
2. 创建具体的 View 实现（EditorView, PreviewView 等）
3. 创建具体的 PopView 实现（SettingsPopView, AboutPopView 等）
4. 集成到主应用中
5. 实现主题切换 UI
