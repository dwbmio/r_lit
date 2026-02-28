use crate::gui::{Page, Theme};
use crate::user_db::UserInfo;
use gpui::{div, prelude::*, IntoElement, Window, Context, px, rgb};

/// 群组发现页面
///
/// 显示本地网络中发现的所有群组
pub struct GroupDiscoveryPage {
    current_user: UserInfo,
    discovered_groups: Vec<GroupInfo>,
    is_discovering: bool,
}

/// 群组信息
#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub id: String,
    pub member_count: usize,
}

impl GroupDiscoveryPage {
    pub fn new(current_user: UserInfo) -> Self {
        Self {
            current_user,
            discovered_groups: Vec::new(),
            is_discovering: false,
        }
    }

    pub fn set_discovering(&mut self, discovering: bool) {
        self.is_discovering = discovering;
    }

    pub fn is_discovering(&self) -> bool {
        self.is_discovering
    }

    pub fn set_groups(&mut self, groups: Vec<GroupInfo>) {
        self.discovered_groups = groups;
    }

    pub fn groups(&self) -> &[GroupInfo] {
        &self.discovered_groups
    }
}

impl Page for GroupDiscoveryPage {
    fn id(&self) -> &'static str {
        "group_discovery"
    }

    fn title(&self) -> Option<String> {
        Some("发现群组".to_string())
    }

    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement
    where
        Self: Sized
    {
        let theme = Theme::default();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme.colors.background))
            // 顶部栏
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(64.0))
                    .px(theme.spacing.xl)
                    .bg(rgb(theme.colors.surface))
                    .border_b_1()
                    .border_color(rgb(theme.colors.border))
                    .child(
                        div()
                            .text_size(theme.typography.heading.size)
                            .text_color(rgb(theme.colors.text))
                            .child("🔍 发现群组")
                    )
                    .child(
                        div()
                            .text_size(theme.typography.body.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child(format!("你好, {}", self.current_user.nickname))
                    )
            )
            // 主内容区
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .p(theme.spacing.xxl)
                    .gap(theme.spacing.xl)
                    // 说明文字
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(theme.spacing.sm)
                            .child(
                                div()
                                    .text_size(px(64.0))
                                    .child("📡")
                            )
                            .child(
                                div()
                                    .text_size(theme.typography.subheading.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child("搜索本地网络中的群组")
                            )
                            .child(
                                div()
                                    .text_size(theme.typography.body.size)
                                    .text_color(rgb(theme.colors.text_secondary))
                                    .child("自动发现同一局域网内的协作群组")
                            )
                    )
                    // 搜索按钮
                    .child(
                        div()
                            .px(theme.spacing.xl)
                            .py(theme.spacing.md)
                            .bg(rgb(
                                if self.is_discovering {
                                    theme.colors.surface_variant
                                } else {
                                    theme.colors.primary
                                }
                            ))
                            .rounded(theme.radius.md)
                            .cursor_pointer()
                            .when(!self.is_discovering, |this| {
                                this.hover(|style| style.bg(rgb(theme.colors.primary_hover)))
                            })
                            .child(
                                div()
                                    .text_size(theme.typography.button.size)
                                    .text_color(rgb(0xffffff))
                                    .child(
                                        if self.is_discovering {
                                            "🔄 正在搜索..."
                                        } else {
                                            "🔍 开始搜索"
                                        }
                                    )
                            )
                    )
                    // 群组列表
                    .child(
                        div()
                            .w_full()
                            .max_w(px(600.0))
                            .flex()
                            .flex_col()
                            .gap(theme.spacing.md)
                            .child(
                                if self.discovered_groups.is_empty() {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap(theme.spacing.sm)
                                        .p(theme.spacing.xl)
                                        .child(
                                            div()
                                                .text_size(theme.typography.body.size)
                                                .text_color(rgb(theme.colors.text_disabled))
                                                .child(
                                                    if self.is_discovering {
                                                        "搜索中，请稍候..."
                                                    } else {
                                                        "点击上方按钮开始搜索"
                                                    }
                                                )
                                        )
                                } else {
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(theme.spacing.md)
                                        .child(
                                            div()
                                                .text_size(theme.typography.body.size)
                                                .text_color(rgb(theme.colors.text_secondary))
                                                .child(format!("发现 {} 个群组", self.discovered_groups.len()))
                                        )
                                        .children(
                                            self.discovered_groups.iter().map(|group| {
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .p(theme.spacing.lg)
                                                    .bg(rgb(theme.colors.surface))
                                                    .rounded(theme.radius.md)
                                                    .border_1()
                                                    .border_color(rgb(theme.colors.border))
                                                    .cursor_pointer()
                                                    .hover(|style| {
                                                        style
                                                            .bg(rgb(theme.colors.surface_variant))
                                                            .border_color(rgb(theme.colors.primary))
                                                    })
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .gap(theme.spacing.md)
                                                            .child(
                                                                div()
                                                                    .w(px(48.0))
                                                                    .h(px(48.0))
                                                                    .bg(rgb(theme.colors.primary))
                                                                    .rounded(theme.radius.md)
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_center()
                                                                    .child(
                                                                        div()
                                                                            .text_size(px(24.0))
                                                                            .child("🏠")
                                                                    )
                                                            )
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .flex_col()
                                                                    .gap(theme.spacing.xs)
                                                                    .child(
                                                                        div()
                                                                            .text_size(theme.typography.subheading.size)
                                                                            .text_color(rgb(theme.colors.text))
                                                                            .child(format!("群组: {}", group.id))
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .text_size(theme.typography.caption.size)
                                                                            .text_color(rgb(theme.colors.text_secondary))
                                                                            .child(format!("{} 人在线", group.member_count))
                                                                    )
                                                            )
                                                    )
                                                    .child(
                                                        div()
                                                            .px(theme.spacing.lg)
                                                            .py(theme.spacing.sm)
                                                            .bg(rgb(theme.colors.primary))
                                                            .rounded(theme.radius.sm)
                                                            .child(
                                                                div()
                                                                    .text_size(theme.typography.caption.size)
                                                                    .text_color(rgb(0xffffff))
                                                                    .child("加入")
                                                            )
                                                    )
                                            })
                                        )
                                }
                            )
                    )
            )
    }
}
