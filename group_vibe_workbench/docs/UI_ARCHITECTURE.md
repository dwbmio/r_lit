# UI 架构设计 - 层级管理系统

## 层级结构

```
Application
  └── PageStack (栈管理)
       ├── Page 1 (底层)
       ├── Page 2
       └── Page 3 (当前显示)
            ├── View 1
            ├── View 2
            └── View 3
  └── PopViewStack (弹窗栈)
       ├── PopView Layer 1 (底层弹窗)
       └── PopView Layer 2 (顶层弹窗)
  └── ToastQueue (固定速率队列)
       ├── Toast 1 (正在显示)
       ├── Toast 2 (等待)
       └── Toast 3 (等待)
```

## 1. Page 管理（栈模式）

### 特点
- **独立存在**: 每个 Page 是完整的页面
- **栈管理**: 支持 push/pop/replace
- **生命周期**: onCreate, onResume, onPause, onDestroy
- **状态保持**: 被压栈的 Page 保持状态

### 实现

```rust
pub struct PageStack {
    pages: Vec<Box<dyn Page>>,
}

impl PageStack {
    /// 压入新页面
    pub fn push(&mut self, page: Box<dyn Page>) {
        if let Some(current) = self.pages.last_mut() {
            current.on_pause();
        }
        page.on_create();
        page.on_resume();
        self.pages.push(page);
    }

    /// 弹出当前页面
    pub fn pop(&mut self) -> Option<Box<dyn Page>> {
        if self.pages.len() <= 1 {
            return None; // 保留至少一个页面
        }

        let page = self.pages.pop();
        if let Some(page) = page.as_ref() {
            page.on_destroy();
        }

        if let Some(current) = self.pages.last_mut() {
            current.on_resume();
        }

        page
    }

    /// 替换当前页面
    pub fn replace(&mut self, page: Box<dyn Page>) {
        if let Some(old) = self.pages.pop() {
            old.on_destroy();
        }
        page.on_create();
        page.on_resume();
        self.pages.push(page);
    }

    /// 清空栈并设置根页面
    pub fn reset(&mut self, page: Box<dyn Page>) {
        for old in self.pages.drain(..) {
            old.on_destroy();
        }
        page.on_create();
        page.on_resume();
        self.pages.push(page);
    }

    /// 获取当前页面
    pub fn current(&self) -> Option<&dyn Page> {
        self.pages.last().map(|p| p.as_ref())
    }

    /// 获取当前页面（可变）
    pub fn current_mut(&mut self) -> Option<&mut dyn Page> {
        self.pages.last_mut().map(|p| p.as_mut())
    }
}
```

### Page Trait

```rust
pub trait Page {
    fn id(&self) -> &'static str;
    fn title(&self) -> Option<String> { None }

    // 生命周期
    fn on_create(&mut self) {}
    fn on_resume(&mut self) {}
    fn on_pause(&mut self) {}
    fn on_destroy(&mut self) {}

    // 渲染
    fn render(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement;

    // 返回键处理
    fn on_back(&mut self) -> bool { true } // true = 允许返回
}
```

## 2. View 管理（父子关系）

### 特点
- **独立存在**: 每个 View 是独立的组件
- **父子关系**: View 可以包含子 View
- **状态管理**: 每个 View 管理自己的状态
- **事件传递**: 支持事件冒泡

### 实现

```rust
pub trait View {
    fn id(&self) -> &str;

    // 渲染
    fn render(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement;

    // 事件处理
    fn on_event(&mut self, event: &Event) -> bool { false }

    // 子 View 管理
    fn children(&self) -> Vec<&dyn View> { vec![] }
}

// 示例：群组发现页面
pub struct GroupDiscoveryPage {
    id: String,
    views: Vec<Box<dyn View>>,
    // ... 其他状态
}

impl Page for GroupDiscoveryPage {
    fn render(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            // 渲染所有子 View
            .children(
                self.views.iter().map(|view| view.render(theme, cx))
            )
    }
}
```

## 3. PopView 管理（层级栈）

### 特点
- **层级管理**: 支持多层弹窗叠加
- **同层单一**: 同一层级只能有一个 PopView
- **遮罩层**: 每层都有自己的遮罩
- **优先级**: 高层级优先处理事件

### 实现

```rust
pub struct PopViewStack {
    layers: Vec<PopViewLayer>,
}

pub struct PopViewLayer {
    level: u32,           // 层级（0 = 最底层）
    popview: Box<dyn PopView>,
    mask_opacity: f32,    // 遮罩透明度
}

impl PopViewStack {
    /// 显示弹窗（指定层级）
    pub fn show(&mut self, popview: Box<dyn PopView>, level: u32) {
        // 移除同层级的旧弹窗
        self.layers.retain(|layer| layer.level != level);

        // 添加新弹窗
        self.layers.push(PopViewLayer {
            level,
            popview,
            mask_opacity: 0.5,
        });

        // 按层级排序
        self.layers.sort_by_key(|layer| layer.level);
    }

    /// 关闭指定层级的弹窗
    pub fn dismiss(&mut self, level: u32) {
        self.layers.retain(|layer| layer.level != level);
    }

    /// 关闭顶层弹窗
    pub fn dismiss_top(&mut self) {
        self.layers.pop();
    }

    /// 关闭所有弹窗
    pub fn dismiss_all(&mut self) {
        self.layers.clear();
    }

    /// 渲染所有弹窗
    pub fn render(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .size_full()
            .children(
                self.layers.iter().map(|layer| {
                    div()
                        .absolute()
                        .size_full()
                        // 遮罩层
                        .child(
                            div()
                                .absolute()
                                .size_full()
                                .bg(rgb(0x000000))
                                .opacity(layer.mask_opacity)
                        )
                        // 弹窗内容
                        .child(layer.popview.render(theme, cx))
                })
            )
    }
}
```

### PopView Trait

```rust
pub trait PopView {
    fn id(&self) -> &str;
    fn level(&self) -> u32 { 0 } // 默认层级

    // 渲染
    fn render(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement;

    // 关闭回调
    fn on_dismiss(&mut self) {}

    // 点击遮罩是否关闭
    fn dismiss_on_mask_click(&self) -> bool { true }
}
```

### 层级定义

```rust
pub mod PopViewLevel {
    pub const DIALOG: u32 = 0;      // 对话框
    pub const MENU: u32 = 1;        // 菜单
    pub const TOOLTIP: u32 = 2;     // 提示
    pub const LOADING: u32 = 3;     // 加载中（最高优先级）
}
```

## 4. Toast 管理（固定速率队列）

### 特点
- **队列管理**: FIFO 队列
- **固定速率**: 每个 Toast 显示固定时间
- **自动消失**: 超时自动移除
- **位置固定**: 通常在顶部或底部

### 实现

```rust
pub struct ToastQueue {
    toasts: VecDeque<ToastItem>,
    max_visible: usize,      // 最多同时显示几个
    default_duration: Duration, // 默认显示时长
}

pub struct ToastItem {
    toast: Toast,
    created_at: Instant,
    duration: Duration,
}

impl ToastQueue {
    pub fn new() -> Self {
        Self {
            toasts: VecDeque::new(),
            max_visible: 3,
            default_duration: Duration::from_secs(3),
        }
    }

    /// 添加 Toast
    pub fn push(&mut self, toast: Toast) {
        self.toasts.push_back(ToastItem {
            toast,
            created_at: Instant::now(),
            duration: self.default_duration,
        });
    }

    /// 更新队列（移除过期的）
    pub fn update(&mut self) {
        let now = Instant::now();
        self.toasts.retain(|item| {
            now.duration_since(item.created_at) < item.duration
        });
    }

    /// 获取当前可见的 Toast
    pub fn visible(&self) -> impl Iterator<Item = &Toast> {
        self.toasts
            .iter()
            .take(self.max_visible)
            .map(|item| &item.toast)
    }

    /// 渲染
    pub fn render(&self, theme: &Theme) -> impl IntoElement {
        div()
            .absolute()
            .top(px(20.0))
            .right(px(20.0))
            .flex()
            .flex_col()
            .gap(theme.spacing.sm)
            .children(
                self.visible().map(|toast| toast.render(theme))
            )
    }
}
```

## 5. 完整的 WorkbenchView 架构

```rust
pub struct WorkbenchView {
    // 页面栈
    page_stack: PageStack,

    // 弹窗栈
    popview_stack: PopViewStack,

    // Toast 队列
    toast_queue: ToastQueue,

    // 全局状态
    current_user: UserInfo,
}

impl WorkbenchView {
    pub fn new(initial_page: Box<dyn Page>) -> Self {
        let mut page_stack = PageStack::new();
        page_stack.push(initial_page);

        Self {
            page_stack,
            popview_stack: PopViewStack::new(),
            toast_queue: ToastQueue::new(),
            current_user: UserInfo::default(),
        }
    }

    // 页面导航
    pub fn navigate_to(&mut self, page: Box<dyn Page>) {
        self.page_stack.push(page);
    }

    pub fn go_back(&mut self) -> bool {
        if let Some(current) = self.page_stack.current_mut() {
            if !current.on_back() {
                return false; // 页面拦截了返回
            }
        }
        self.page_stack.pop().is_some()
    }

    pub fn replace_page(&mut self, page: Box<dyn Page>) {
        self.page_stack.replace(page);
    }

    // 弹窗管理
    pub fn show_dialog(&mut self, dialog: Box<dyn PopView>) {
        self.popview_stack.show(dialog, PopViewLevel::DIALOG);
    }

    pub fn show_loading(&mut self, loading: Box<dyn PopView>) {
        self.popview_stack.show(loading, PopViewLevel::LOADING);
    }

    pub fn dismiss_loading(&mut self) {
        self.popview_stack.dismiss(PopViewLevel::LOADING);
    }

    // Toast 管理
    pub fn show_toast(&mut self, toast: Toast) {
        self.toast_queue.push(toast);
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::default();

        // 更新 Toast 队列
        self.toast_queue.update();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme.colors.background))
            // 当前页面
            .child(
                self.page_stack.current_mut()
                    .map(|page| page.render(&theme, cx))
                    .unwrap_or_else(|| div().child("No page"))
            )
            // 弹窗层
            .child(self.popview_stack.render(&theme, cx))
            // Toast 层
            .child(self.toast_queue.render(&theme))
    }
}
```

## 6. 使用示例

### 页面导航

```rust
// 从登录页跳转到群组发现页
workbench.navigate_to(Box::new(GroupDiscoveryPage::new(user)));

// 从群组发现页跳转到群组大厅
workbench.navigate_to(Box::new(GroupLobbyPage::new(user, group_id)));

// 返回上一页
workbench.go_back();

// 替换当前页（不保留历史）
workbench.replace_page(Box::new(LoginPage::new()));
```

### 弹窗管理

```rust
// 显示确认对话框
workbench.show_dialog(Box::new(ConfirmDialog::new(
    "确认退出？",
    "退出后将断开与群组的连接",
)));

// 显示加载中
workbench.show_loading(Box::new(LoadingDialog::new("正在搜索群组...")));

// 关闭加载中
workbench.dismiss_loading();
```

### Toast 提示

```rust
// 成功提示
workbench.show_toast(Toast::success("加入群组成功"));

// 错误提示
workbench.show_toast(Toast::error("网络连接失败"));

// 信息提示
workbench.show_toast(Toast::info("正在搜索群组..."));
```

## 7. 优势

### 清晰的层级
- Page 管理页面流程
- View 管理页面内组件
- PopView 管理弹窗
- Toast 管理提示

### 独立性
- 每个层级独立管理
- 互不干扰
- 易于测试

### 可扩展性
- 新增 Page 只需实现 trait
- 新增 PopView 只需实现 trait
- 新增 Toast 类型很简单

### 状态管理
- Page 栈保持状态
- PopView 层级清晰
- Toast 自动过期

## 8. 下一步实现

1. 实现 PageStack
2. 实现 PopViewStack
3. 重构现有代码使用新架构
4. 添加页面转场动画
5. 添加弹窗动画
