use gpui::{div, IntoElement, Window, Context, px, rgb};

/// PopView - 浮层弹窗，互斥显示
///
/// PopView 是浮动在页面之上的弹窗组件，同一时间只能显示一个 PopView。
/// 新的 PopView 打开时，会自动关闭当前显示的 PopView。
///
/// # 特性
/// - 互斥显示：同时只能有一个 PopView 可见
/// - 遮罩层：可选的半透明背景遮罩
/// - 居中显示：默认在屏幕中央显示
/// - 可关闭：支持点击遮罩或 ESC 键关闭
///
/// # 示例
/// ```rust
/// struct SettingsPopView {
///     settings: Settings,
/// }
///
/// impl PopView for SettingsPopView {
///     fn id(&self) -> &'static str {
///         "settings"
///     }
///
///     fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         div()
///             .w(px(600.0))
///             .h(px(400.0))
///             .bg(rgb(0x313244))
///             .child("Settings Content")
///     }
/// }
/// ```
pub trait PopView {
    /// PopView 唯一标识符
    fn id(&self) -> &'static str;

    /// PopView 标题
    fn title(&self) -> Option<String> {
        None
    }

    /// 渲染 PopView 内容
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized;

    /// 是否显示遮罩层
    fn show_mask(&self) -> bool {
        true
    }

    /// 是否可以通过点击遮罩关闭
    fn closable_by_mask(&self) -> bool {
        true
    }

    /// 是否可以通过 ESC 键关闭
    fn closable_by_esc(&self) -> bool {
        true
    }

    /// PopView 打开时的生命周期钩子
    fn on_open(&mut self) {}

    /// PopView 关闭时的生命周期钩子
    fn on_close(&mut self) {}

    /// PopView 关闭前的确认（返回 false 阻止关闭）
    fn before_close(&self) -> bool {
        true
    }
}

/// PopViewManager - PopView 管理器
///
/// 管理 PopView 的显示和隐藏，确保互斥显示
pub struct PopViewManager {
    current_popview: Option<String>,
}

impl PopViewManager {
    pub fn new() -> Self {
        Self {
            current_popview: None,
        }
    }

    /// 打开一个 PopView
    pub fn open(&mut self, id: String) {
        self.current_popview = Some(id);
    }

    /// 关闭当前 PopView
    pub fn close(&mut self) {
        self.current_popview = None;
    }

    /// 获取当前显示的 PopView ID
    pub fn current(&self) -> Option<&str> {
        self.current_popview.as_deref()
    }

    /// 检查指定 PopView 是否正在显示
    pub fn is_showing(&self, id: &str) -> bool {
        self.current_popview.as_deref() == Some(id)
    }
}

impl Default for PopViewManager {
    fn default() -> Self {
        Self::new()
    }
}

/// PopViewContainer - PopView 的容器包装器
///
/// 提供遮罩层和居中布局
pub struct PopViewContainer<P: PopView> {
    popview: P,
    visible: bool,
}

impl<P: PopView> PopViewContainer<P> {
    pub fn new(popview: P) -> Self {
        Self {
            popview,
            visible: false,
        }
    }

    pub fn popview(&self) -> &P {
        &self.popview
    }

    pub fn popview_mut(&mut self) -> &mut P {
        &mut self.popview
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.popview.on_open();
    }

    pub fn hide(&mut self) {
        if self.popview.before_close() {
            self.visible = false;
            self.popview.on_close();
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }
}
