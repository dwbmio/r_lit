use crate::gui::{Page, Theme};
use crate::user_db::UserInfo;
use gpui::{div, prelude::*, IntoElement, Window, Context, px, rgb};

/// 群组选择页面
///
/// 让用户输入群组 ID 或创建新群组
pub struct GroupDiscoveryPage {
    current_user: UserInfo,
    group_id_input: String,
}

impl GroupDiscoveryPage {
    pub fn new(current_user: UserInfo) -> Self {
        Self {
            current_user,
            group_id_input: String::new(),
        }
    }

    pub fn set_group_id(&mut self, group_id: String) {
        self.group_id_input = group_id;
    }

    pub fn group_id(&self) -> &str {
        &self.group_id_input
    }
}

impl Page for GroupDiscoveryPage {
    fn id(&self) -> &'static str {
        "group_discovery"
    }

    fn title(&self) -> String {
        "选择或创建群组".to_string()
    }

    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::default();

        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .bg(theme.background)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_8()
                    .bg(theme.surface)
                    .rounded(px(12.0))
                    .shadow_lg()
                    .child(
                        div()
                            .text_2xl()
                            .text_color(theme.text)
                            .child(format!("欢迎, {}!", self.current_user.nickname))
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.subtext)
                            .child("输入群组 ID 加入现有群组，或创建新群组")
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.text)
                                    .child("群组 ID:")
                            )
                            .child(
                                div()
                                    .w(px(300.0))
                                    .h(px(40.0))
                                    .bg(theme.background)
                                    .border_1()
                                    .border_color(theme.border)
                                    .rounded(px(6.0))
                                    .px_3()
                                    .flex()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.subtext)
                                            .child(if self.group_id_input.is_empty() {
                                                "输入群组 ID 或留空自动生成"
                                            } else {
                                                &self.group_id_input
                                            })
                                    )
                            )
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                div()
                                    .px_6()
                                    .py_2()
                                    .bg(theme.primary)
                                    .text_color(theme.text)
                                    .rounded(px(6.0))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x89b4fa)))
                                    .child("加入群组")
                            )
                            .child(
                                div()
                                    .px_6()
                                    .py_2()
                                    .bg(theme.surface_bright)
                                    .text_color(theme.text)
                                    .rounded(px(6.0))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x45475a)))
                                    .child("创建新群组")
                            )
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.subtext)
                            .child("提示: 同一群组的成员会自动发现并连接")
                    )
            )
    }
}
