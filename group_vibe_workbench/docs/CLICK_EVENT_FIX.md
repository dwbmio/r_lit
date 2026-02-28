# 修复按钮点击事件

## 问题

所有按钮都无法点击，包括：
- 登录页面的"使用默认昵称"和"开始使用"按钮
- 群组发现页面的"搜索群组"和"创建新群组"按钮
- 群组列表的"加入"按钮

## 根本原因

GPUI 0.2.2 的事件处理 API 与最新版本不同：

1. **没有 `on_click` 方法**: 需要使用 `on_mouse_down`
2. **需要 `MouseButton` 参数**: `on_mouse_down(MouseButton::Left, handler)`
3. **需要 `cx.listener()`**: 通过 `Context` 创建事件监听器
4. **需要 `cx.notify()`**: 通知 GPUI 重新渲染

## 解决方案

### 1. 修改 render 方法签名

```rust
// 之前
fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement

// 之后
fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
```

### 2. 传递 cx 到子渲染方法

```rust
// 之前
fn render_login_overlay(&self, theme: &Theme) -> impl IntoElement
fn render_group_discovery(&self, theme: &Theme) -> impl IntoElement

// 之后
fn render_login_overlay(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement
fn render_group_discovery(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement
```

### 3. 添加点击事件

```rust
// 登录按钮
div()
    .px(theme.spacing.xl)
    .py(theme.spacing.md)
    .bg(rgb(theme.colors.primary))
    .rounded(theme.radius.md)
    .cursor_pointer()
    .hover(|style| style.bg(rgb(0x89b4fa)))
    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
        this.use_default_nickname();
        cx.notify();
    }))
    .child(/* ... */)
```

### 4. 群组列表点击事件

```rust
groups.iter().map(|group| {
    let group_id = group.id.clone();  // 克隆以便在闭包中使用
    div()
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _, cx| {
            this.join_group(group_id.clone());
            cx.notify();
        }))
        .child(/* ... */)
})
```

## GPUI 0.2.2 事件处理 API

### 鼠标事件

```rust
// 鼠标按下
.on_mouse_down(MouseButton::Left, cx.listener(|this, event, window, cx| {
    // 处理点击
    cx.notify();  // 触发重新渲染
}))

// 鼠标释放
.on_mouse_up(MouseButton::Left, cx.listener(|this, event, window, cx| {
    // 处理释放
}))

// 鼠标移动
.on_mouse_move(cx.listener(|this, event, window, cx| {
    // 处理移动
}))
```

### 事件监听器参数

```rust
cx.listener(|this, event, window, cx| {
    // this: &mut Self - View 的可变引用
    // event: &MouseDownEvent - 事件数据
    // window: &mut Window - 窗口引用
    // cx: &mut Context<Self> - 上下文
})
```

### 通知重新渲染

```rust
cx.notify();  // 标记 View 需要重新渲染
```

## 实现的功能

### 登录页面
- ✅ "使用默认昵称" 按钮 → 生成 `User_xxxxxxxx` 并登录
- ✅ "开始使用" 按钮 → 使用当前输入的昵称登录（如果为空则提示）

### 群组发现页面
- ✅ "搜索群组" 按钮 → 启动 mDNS 群组发现
- ✅ "创建新群组" 按钮 → 生成新群组 ID 并加入
- ✅ 群组列表项点击 → 加入选中的群组

## 测试

### 1. 测试登录
```bash
./target/release/group_vibe_workbench launch
# 点击"使用默认昵称"按钮
# 应该自动生成昵称并进入群组发现页面
```

### 2. 测试群组创建
```bash
./target/release/group_vibe_workbench launch -n "Alice"
# 点击"创建新群组"按钮
# 应该创建群组并进入群组大厅
```

### 3. 测试群组发现
```bash
# 终端 1
./target/release/group_vibe_workbench launch -n "Alice"
# 点击"创建新群组"

# 终端 2
./target/release/group_vibe_workbench launch -n "Bob"
# 点击"搜索群组"
# 应该看到 Alice 的群组
# 点击群组加入
```

## 已知限制

### 1. UI 更新延迟
当前群组发现是异步的，UI 不会自动更新发现的群组列表。需要手动刷新或实现消息传递机制。

### 2. 没有加载状态
点击"搜索群组"后，没有明显的加载指示器（虽然有 `is_discovering` 状态，但 UI 更新有延迟）。

### 3. 错误处理
网络错误或群组加入失败时，没有明显的错误提示（虽然有 Toast，但可能不够明显）。

## 下一步改进

### 1. 实现 UI 更新机制
使用 `cx.spawn()` 或消息传递来更新 UI：

```rust
cx.spawn(|this, mut cx| async move {
    let groups = discover_groups().await;
    cx.update(|cx| {
        this.update(cx, |this, cx| {
            this.update_groups(groups);
            cx.notify();
        });
    });
})
```

### 2. 添加加载动画
在搜索时显示旋转的加载图标。

### 3. 改进错误提示
使用更明显的 Toast 或对话框显示错误。

### 4. 添加群组退出功能
在群组大厅添加"退出群组"按钮。

## 参考

- GPUI 0.2.2 examples: `~/.cargo/registry/src/.../gpui-0.2.2/examples/`
- 特别参考: `input.rs`, `gradient.rs`, `image_loading.rs`

## 总结

现在所有按钮都可以点击了！关键是：

1. ✅ 使用 `on_mouse_down(MouseButton::Left, cx.listener(...))`
2. ✅ 在闭包中调用 `cx.notify()` 触发重新渲染
3. ✅ 通过 `cx.listener()` 创建事件监听器
4. ✅ 在 `map()` 中使用 `clone()` 避免生命周期问题
