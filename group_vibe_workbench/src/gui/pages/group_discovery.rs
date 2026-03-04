use crate::gui::{Page, Theme};
use crate::user_db::UserInfo;
use gpui::{prelude::*, Entity, IntoElement, Window, Context, div, px, rgb};
use gpui_component::input::{Input, InputState};

/// 群组选择页面
///
/// 让用户输入群组 ID 或创建新群组
pub struct GroupDiscoveryPage {
    pub current_user: UserInfo,
    pub group_id_input: Entity<InputState>,
}

impl GroupDiscoveryPage {
    pub fn new(current_user: UserInfo, group_id_input: Entity<InputState>) -> Self {
        Self {
            current_user,
            group_id_input,
        }
    }

    pub fn group_id_text(&self, cx: &gpui::App) -> String {
        self.group_id_input.read(cx).text().to_string()
    }
}

impl Page for GroupDiscoveryPage {
    fn id(&self) -> &'static str {
        "group_discovery"
    }

    fn title(&self) -> Option<String> {
        Some("选择或创建群组".to_string())
    }

    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::default();

        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .size_full()
            .bg(rgb(theme.colors.background))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_8()
                    .bg(rgb(theme.colors.surface))
                    .rounded(px(12.0))
                    .shadow_lg()
                    .child(
                        div()
                            .text_2xl()
                            .text_color(rgb(theme.colors.text))
                            .child(format!("欢迎, {}!", self.current_user.nickname))
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme.colors.text_secondary))
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
                                    .text_color(rgb(theme.colors.text))
                                    .child("群组 ID:")
                            )
                            .child(
                                div()
                                    .w(px(300.0))
                                    .child(Input::new(&self.group_id_input).cleanable(true))
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
                                    .bg(rgb(theme.colors.primary))
                                    .text_color(rgb(0xffffff))
                                    .rounded(px(6.0))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x74c7ec)))
                                    .child("加入群组")
                            )
                            .child(
                                div()
                                    .px_6()
                                    .py_2()
                                    .bg(rgb(theme.colors.surface_variant))
                                    .text_color(rgb(theme.colors.text))
                                    .rounded(px(6.0))
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x45475a)))
                                    .child("创建新群组")
                            )
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("提示: 同一群组的成员会自动发现并连接")
                    )
            )
    }
}
