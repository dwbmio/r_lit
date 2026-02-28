use crate::gui::{PopView, Theme};
use gpui::{div, prelude::*, IntoElement, Window, Context, px, rgb};

/// 简单的加载动画弹窗
pub struct LoadingPopView {
    message: String,
    closable: bool,
}

impl LoadingPopView {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            closable: false,
        }
    }

    pub fn with_closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    pub fn is_closable(&self) -> bool {
        self.closable
    }
}

impl PopView for LoadingPopView {
    fn id(&self) -> &'static str {
        "loading"
    }

    fn title(&self) -> Option<String> {
        None
    }

    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::default();

        div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .child(
                div()
                    .w(px(400.0))
                    .h(px(300.0))
                    .bg(rgb(theme.colors.surface))
                    .rounded(theme.radius.lg)
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(theme.spacing.xl)
                    // 旋转的加载图标
                    .child(
                        div()
                            .text_size(px(64.0))
                            .child("⏳")
                    )
                    // 加载消息
                    .child(
                        div()
                            .text_size(theme.typography.subheading.size)
                            .text_color(rgb(theme.colors.text))
                            .child(self.message.clone())
                    )
                    // 进度条
                    .child(
                        div()
                            .w(px(300.0))
                            .h(px(4.0))
                            .bg(rgb(theme.colors.surface_variant))
                            .rounded(px(2.0))
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(px(100.0))
                                    .bg(rgb(theme.colors.primary))
                                    .rounded(px(2.0))
                            )
                    )
                    // 提示文字
                    .child(
                        div()
                            .text_size(theme.typography.caption.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("预计需要 5-8 秒...")
                    )
            )
    }

    fn closable_by_mask(&self) -> bool {
        self.closable // 根据设置决定是否允许点击遮罩关闭
    }

    fn closable_by_esc(&self) -> bool {
        self.closable // 根据设置决定是否允许 ESC 关闭
    }
}
