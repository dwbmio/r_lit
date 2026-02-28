use gpui::{div, IntoElement, Window, Context};

/// View - Page 上的全屏区域
///
/// View 是 Page 内部的大块内容区域，通常占据整个 Page 或 Page 的主要部分。
/// 一个 Page 可以包含多个 View，但通常同时只显示一个主 View。
///
/// # 与 Page 的区别
/// - Page 是路由级别的容器，View 是 Page 内部的内容区域
/// - Page 切换会改变 URL，View 切换不会
/// - View 可以在同一个 Page 内切换（如 Tab 切换）
///
/// # 示例
/// ```rust
/// struct EditorView {
///     content: String,
/// }
///
/// impl View for EditorView {
///     fn id(&self) -> &'static str {
///         "editor"
///     }
///
///     fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         div()
///             .flex()
///             .flex_col()
///             .size_full()
///             .child(&self.content)
///     }
/// }
/// ```
pub trait View {
    /// View 唯一标识符
    fn id(&self) -> &'static str;

    /// View 标题（可选，用于 Tab 等）
    fn title(&self) -> Option<String> {
        None
    }

    /// 渲染 View 内容
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized;

    /// View 激活时的生命周期钩子
    fn on_activate(&mut self) {}

    /// View 失活时的生命周期钩子
    fn on_deactivate(&mut self) {}

    /// View 是否可见
    fn is_visible(&self) -> bool {
        true
    }
}

/// ViewContainer - View 的容器包装器
///
/// 提供统一的 View 容器样式和布局
pub struct ViewContainer<V: View> {
    view: V,
    visible: bool,
}

impl<V: View> ViewContainer<V> {
    pub fn new(view: V) -> Self {
        Self {
            view,
            visible: true,
        }
    }

    pub fn view(&self) -> &V {
        &self.view
    }

    pub fn view_mut(&mut self) -> &mut V {
        &mut self.view
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible && self.view.is_visible()
    }
}
