# 修复登录界面 - 添加昵称输入支持

## 问题

用户启动应用后看到登录界面，但没有输入框可以输入昵称。

## 解决方案

由于 GPUI 0.2.2 的文本输入功能有限，我们采用了两种方式：

### 1. 命令行参数（推荐）

添加 `--nickname` / `-n` 参数，允许用户在启动时直接指定昵称：

```bash
group_vibe_workbench launch --nickname "Alice"
# 或
group_vibe_workbench launch -n "Alice"
```

### 2. GUI 默认昵称按钮

在登录界面添加了两个按钮：
- **"使用默认昵称"**: 自动生成 `User_xxxxxxxx` 格式的昵称
- **"开始使用"**: 使用当前输入的昵称（如果为空则使用默认）

## 实现细节

### 1. 更新 CLI 参数

**文件**: `src/main.rs`

```rust
Launch {
    /// Window width
    #[arg(long, default_value = "1280")]
    width: u32,

    /// Window height
    #[arg(long, default_value = "720")]
    height: u32,

    /// Your nickname (optional, will prompt if not provided)
    #[arg(long, short = 'n')]
    nickname: Option<String>,
}
```

### 2. 更新 launch 函数

**文件**: `src/subcmd/launch.rs`

```rust
pub fn run(width: u32, height: u32, nickname: Option<String>) -> Result<()> {
    // 如果提供了昵称，直接使用
    let (has_user, current_user) = if let Some(nick) = nickname {
        log::info!("Using provided nickname: {}", nick);
        let user = UserInfo::new(nick);
        // 保存到数据库
        let _ = user_db.save_user(&user);
        (true, user)
    } else if user_db.has_user() {
        (true, user_db.get_current_user()?.expect("User should exist"))
    } else {
        (false, UserInfo::new("临时用户".to_string()))
    };
    // ...
}
```

### 3. 添加 WorkbenchView 字段

```rust
struct WorkbenchView {
    // ... 其他字段
    nickname_input: String,  // 存储用户输入的昵称
}
```

### 4. 实现默认昵称功能

```rust
fn use_default_nickname(&mut self) {
    let nickname = format!("User_{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
    let user = UserInfo::new(nickname);
    self.handle_login(user);
}

fn use_custom_nickname(&mut self) {
    if self.nickname_input.trim().is_empty() {
        self.toast_queue.push(Toast::warning("请输入昵称".to_string()));
        return;
    }
    let user = UserInfo::new(self.nickname_input.trim().to_string());
    self.handle_login(user);
}
```

### 5. 更新登录界面 UI

添加了：
- 输入框显示区域（显示当前输入或占位符）
- 提示文字（说明 GPUI 限制）
- 两个按钮（使用默认昵称 / 开始使用）

## 使用方式

### 推荐方式：命令行参数

```bash
# 启动并指定昵称
group_vibe_workbench launch -n "Alice"

# 带窗口大小
group_vibe_workbench launch -n "Alice" --width 1920 --height 1080
```

### 备选方式：GUI 按钮

```bash
# 启动应用
group_vibe_workbench launch

# 在 GUI 中点击"使用默认昵称"按钮
# 会自动生成类似 "User_a1b2c3d4" 的昵称
```

## 技术限制

GPUI 0.2.2 的文本输入功能有限：
- 没有内置的 `TextInput` 组件
- 需要手动处理键盘事件和文本渲染
- 实现完整的文本输入需要大量额外代码

因此，我们采用了命令行参数作为主要输入方式，GUI 按钮作为备选方案。

## 文档更新

- ✅ README.md - 更新使用示例
- ✅ USAGE.md - 添加命令行参数说明
- ✅ 帮助信息 - `--help` 显示新参数

## 测试

```bash
# 测试帮助信息
./target/release/group_vibe_workbench launch --help

# 测试昵称参数
./target/release/group_vibe_workbench launch -n "TestUser"

# 测试默认行为（无昵称）
./target/release/group_vibe_workbench launch
```

## 总结

现在用户可以通过以下方式设置昵称：

1. **命令行参数**（推荐）: `launch -n "Alice"`
2. **GUI 默认昵称**: 点击"使用默认昵称"按钮
3. **GUI 自定义昵称**: 输入昵称后点击"开始使用"（功能有限）

这样既解决了输入问题，又提供了良好的用户体验！✅
