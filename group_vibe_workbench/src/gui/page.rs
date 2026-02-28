use gpui::{div, prelude::*, IntoElement, Window, Context};

/// Page - 全屏页面，独立路由
///
/// Page 是应用的顶层容器，每个 Page 对应一个独立的路由。
/// 一个应用可以有多个 Page，但同一时间只显示一个。
///
/// # 示例
/// ```rust
/// struct HomePage;
///
/// impl Page for HomePage {
///     fn id(&self) -> &'static str {
///         "home"
///     }
///
///     fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
///         div().child("Home Page Content")
///     }
/// }
/// ```
pub trait Page {
    /// 页面唯一标识符，用于路由
    fn id(&self) -> &'static str;

    /// 页面标题（可选）
    fn title(&self) -> Option<String> {
        None
    }

    /// 渲染页面内容
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized;

    /// 页面进入时的生命周期钩子
    fn on_enter(&mut self) {}

    /// 页面离开时的生命周期钩子
    fn on_leave(&mut self) {}

    /// 页面是否可以离开（用于未保存提示等）
    fn can_leave(&self) -> bool {
        true
    }
}

/// PageContainer - Page 的容器包装器
///
/// 提供统一的页面容器样式和布局
pub struct PageContainer<P: Page> {
    page: P,
}

impl<P: Page> PageContainer<P> {
    pub fn new(page: P) -> Self {
        Self { page }
    }

    pub fn page(&self) -> &P {
        &self.page
    }

    pub fn page_mut(&mut self) -> &mut P {
        &mut self.page
    }
}

impl<P: Page> IntoElement for PageContainer<P> {
    type Element = gpui::Div;

    fn into_element(self) -> Self::Element {
        // PageContainer 的渲染需要在实际使用时通过 Page trait 的 render 方法
        // 这里提供一个占位实现
        div()
    }
}
