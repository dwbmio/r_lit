use crate::gui::{Page, Theme};
use crate::user_db::UserInfo;
use gpui::{div, prelude::*, IntoElement, Window, Context, px, rgb};

/// 群组成员信息
#[derive(Debug, Clone)]
pub struct GroupMember {
    pub id: String,
    pub nickname: String,
    pub avatar: Option<String>,
    pub is_online: bool,
    pub is_leader: bool,
}

/// 群组大厅页面
///
/// 类似麻将游戏，显示群组中所有用户的头像
pub struct GroupLobbyPage {
    pub current_user: UserInfo,
    pub group_id: String,
    pub members: Vec<GroupMember>,
}

impl GroupLobbyPage {
    pub fn new(current_user: UserInfo, group_id: String) -> Self {
        Self {
            current_user,
            group_id,
            members: Vec::new(),
        }
    }

    pub fn add_member(&mut self, member: GroupMember) {
        self.members.push(member);
    }

    pub fn remove_member(&mut self, member_id: &str) {
        self.members.retain(|m| m.id != member_id);
    }

    pub fn update_member_status(&mut self, member_id: &str, is_online: bool) {
        if let Some(member) = self.members.iter_mut().find(|m| m.id == member_id) {
            member.is_online = is_online;
        }
    }

    fn render_member_avatar(&self, member: &GroupMember, theme: &Theme) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(theme.spacing.sm)
            // 头像容器
            .child(
                div()
                    .relative()
                    .child(
                        // 头像
                        div()
                            .w(px(80.0))
                            .h(px(80.0))
                            .bg(rgb(theme.colors.primary))
                            .rounded(theme.radius.full)
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_2()
                            .border_color(rgb(
                                if member.is_online {
                                    theme.colors.success
                                } else {
                                    theme.colors.border
                                }
                            ))
                            .child(
                                div()
                                    .text_size(px(32.0))
                                    .text_color(rgb(0xffffff))
                                    .child(
                                        member.nickname.chars().next().unwrap_or('?').to_string()
                                    )
                            )
                    )
                    .when(member.is_leader, |this| {
                        this.child(
                            // Leader 徽章
                            div()
                                .absolute()
                                .top(px(-4.0))
                                .right(px(-4.0))
                                .w(px(24.0))
                                .h(px(24.0))
                                .bg(rgb(theme.colors.warning))
                                .rounded(theme.radius.full)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_size(px(14.0))
                                        .child("👑")
                                )
                        )
                    })
            )
            // 昵称
            .child(
                div()
                    .text_size(theme.typography.body.size)
                    .text_color(rgb(theme.colors.text))
                    .child(member.nickname.clone())
            )
            // 在线状态
            .child(
                div()
                    .text_size(theme.typography.caption.size)
                    .text_color(rgb(
                        if member.is_online {
                            theme.colors.success
                        } else {
                            theme.colors.text_disabled
                        }
                    ))
                    .child(if member.is_online { "在线" } else { "离线" })
            )
    }
}

impl Page for GroupLobbyPage {
    fn id(&self) -> &'static str {
        "group_lobby"
    }

    fn title(&self) -> Option<String> {
        Some(format!("群组: {}", self.group_id))
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
                    // 左侧：群组信息
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme.spacing.md)
                            .child(
                                div()
                                    .text_size(theme.typography.heading.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child(format!("🏠 群组: {}", self.group_id))
                            )
                            .child(
                                div()
                                    .px(theme.spacing.sm)
                                    .py(theme.spacing.xs)
                                    .bg(rgb(theme.colors.surface_variant))
                                    .rounded(theme.radius.sm)
                                    .text_size(theme.typography.caption.size)
                                    .text_color(rgb(theme.colors.text_secondary))
                                    .child(format!("{} 人在线", self.members.iter().filter(|m| m.is_online).count()))
                            )
                    )
                    // 右侧：当前用户信息
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(theme.spacing.sm)
                            .child(
                                div()
                                    .text_size(theme.typography.body.size)
                                    .text_color(rgb(theme.colors.text_secondary))
                                    .child(format!("你好, {}", self.current_user.nickname))
                            )
                            .child(
                                div()
                                    .w(px(40.0))
                                    .h(px(40.0))
                                    .bg(rgb(theme.colors.primary))
                                    .rounded(theme.radius.full)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        div()
                                            .text_size(px(18.0))
                                            .text_color(rgb(0xffffff))
                                            .child(
                                                self.current_user.nickname.chars().next().unwrap_or('?').to_string()
                                            )
                                    )
                            )
                    )
            )
            // 主内容区：成员网格
            .child(
                div()
                    .flex_1()
                    .p(theme.spacing.xxl)
                    .child(
                        if self.members.is_empty() {
                            // 空状态
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .size_full()
                                .gap(theme.spacing.md)
                                .child(
                                    div()
                                        .text_size(px(64.0))
                                        .child("👥")
                                )
                                .child(
                                    div()
                                        .text_size(theme.typography.subheading.size)
                                        .text_color(rgb(theme.colors.text_secondary))
                                        .child("等待其他成员加入...")
                                )
                                .child(
                                    div()
                                        .text_size(theme.typography.caption.size)
                                        .text_color(rgb(theme.colors.text_disabled))
                                        .child("分享群组 ID 邀请好友加入")
                                )
                        } else {
                            // 成员网格（类似麻将桌）
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(theme.spacing.xl)
                                .justify_center()
                                .children(
                                    self.members.iter().map(|member| {
                                        self.render_member_avatar(member, &theme)
                                    })
                                )
                        }
                    )
            )
            // 底部操作栏
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(80.0))
                    .px(theme.spacing.xl)
                    .bg(rgb(theme.colors.surface))
                    .border_t_1()
                    .border_color(rgb(theme.colors.border))
                    .gap(theme.spacing.md)
                    // 开始协作按钮
                    .child(
                        div()
                            .px(theme.spacing.xl)
                            .py(theme.spacing.md)
                            .bg(rgb(theme.colors.primary))
                            .rounded(theme.radius.md)
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(theme.colors.primary_hover)))
                            .child(
                                div()
                                    .text_size(theme.typography.button.size)
                                    .text_color(rgb(0xffffff))
                                    .child("🚀 开始协作")
                            )
                    )
                    // 邀请成员按钮
                    .child(
                        div()
                            .px(theme.spacing.lg)
                            .py(theme.spacing.md)
                            .bg(rgb(theme.colors.surface_variant))
                            .rounded(theme.radius.md)
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(theme.colors.border)))
                            .child(
                                div()
                                    .text_size(theme.typography.button.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child("➕ 邀请成员")
                            )
                    )
                    // 设置按钮
                    .child(
                        div()
                            .px(theme.spacing.lg)
                            .py(theme.spacing.md)
                            .bg(rgb(theme.colors.surface_variant))
                            .rounded(theme.radius.md)
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(theme.colors.border)))
                            .child(
                                div()
                                    .text_size(theme.typography.button.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child("⚙️ 设置")
                            )
                    )
            )
    }
}
