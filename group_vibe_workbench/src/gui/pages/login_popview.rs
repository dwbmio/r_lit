use crate::gui::{PopView, Theme};
use crate::user_db::UserInfo;
use gpui::{div, prelude::*, IntoElement, Window, Context, px, rgb};

/// 登录弹窗
///
/// 用于首次使用时输入用户信息
pub struct LoginPopView {
    nickname: String,
    error_message: Option<String>,
    on_login: Option<Box<dyn Fn(UserInfo) + Send>>,
}

impl LoginPopView {
    pub fn new() -> Self {
        Self {
            nickname: String::new(),
            error_message: None,
            on_login: None,
        }
    }

    pub fn with_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(UserInfo) + Send + 'static,
    {
        self.on_login = Some(Box::new(callback));
        self
    }

    fn validate_nickname(&self) -> Result<(), String> {
        let nickname = self.nickname.trim();

        if nickname.is_empty() {
            return Err("昵称不能为空".to_string());
        }

        if nickname.len() < 2 {
            return Err("昵称至少需要 2 个字符".to_string());
        }

        if nickname.len() > 20 {
            return Err("昵称不能超过 20 个字符".to_string());
        }

        Ok(())
    }

    fn handle_login(&mut self) {
        match self.validate_nickname() {
            Ok(_) => {
                let user = UserInfo::new(self.nickname.trim().to_string());
                if let Some(callback) = &self.on_login {
                    callback(user);
                }
            }
            Err(err) => {
                self.error_message = Some(err);
            }
        }
    }
}

impl Default for LoginPopView {
    fn default() -> Self {
        Self::new()
    }
}

impl PopView for LoginPopView {
    fn id(&self) -> &'static str {
        "login"
    }

    fn title(&self) -> Option<String> {
        Some("欢迎使用 Group Vibe Workbench".to_string())
    }

    fn show_mask(&self) -> bool {
        true
    }

    fn closable_by_mask(&self) -> bool {
        false // 必须登录才能使用
    }

    fn closable_by_esc(&self) -> bool {
        false // 必须登录才能使用
    }

    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized
    {
        let theme = Theme::default();

        div()
            .w(px(500.0))
            .bg(rgb(theme.colors.surface))
            .rounded(theme.radius.lg)
            .p(theme.spacing.xl)
            .flex()
            .flex_col()
            .gap(theme.spacing.lg)
            // 标题
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme.spacing.sm)
                    .child(
                        div()
                            .text_size(theme.typography.heading.size)
                            .text_color(rgb(theme.colors.text))
                            .child("👋 欢迎")
                    )
                    .child(
                        div()
                            .text_size(theme.typography.body.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("请输入你的昵称开始使用")
                    )
            )
            // 输入框
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme.spacing.sm)
                    .child(
                        div()
                            .text_size(theme.typography.body.size)
                            .text_color(rgb(theme.colors.text))
                            .child("昵称")
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(48.0))
                            .bg(rgb(theme.colors.background))
                            .border_1()
                            .border_color(rgb(theme.colors.border))
                            .rounded(theme.radius.md)
                            .px(theme.spacing.md)
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .text_size(theme.typography.body.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child(
                                        if self.nickname.is_empty() {
                                            "输入你的昵称...".to_string()
                                        } else {
                                            self.nickname.clone()
                                        }
                                    )
                            )
                    )
                    .when(self.error_message.is_some(), |this| {
                        let error_msg = self.error_message.clone().unwrap_or_default();
                        this.child(
                            div()
                                .text_size(theme.typography.caption.size)
                                .text_color(rgb(theme.colors.error))
                                .child(error_msg)
                        )
                    })
            )
            // 提示信息
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(theme.spacing.xs)
                    .child(
                        div()
                            .text_size(theme.typography.caption.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("💡 提示：")
                    )
                    .child(
                        div()
                            .text_size(theme.typography.caption.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("• 昵称将用于在群组中显示")
                    )
                    .child(
                        div()
                            .text_size(theme.typography.caption.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("• 昵称长度为 2-20 个字符")
                    )
            )
            // 按钮
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .px(theme.spacing.lg)
                            .py(theme.spacing.md)
                            .bg(rgb(theme.colors.primary))
                            .rounded(theme.radius.md)
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(theme.colors.primary_hover)))
                            .child(
                                div()
                                    .text_size(theme.typography.button.size)
                                    .text_color(rgb(0xffffff))
                                    .child("开始使用")
                            )
                    )
            )
    }
}
