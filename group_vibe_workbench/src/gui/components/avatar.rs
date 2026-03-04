use crate::gui::Theme;
use gpui::{div, prelude::*, px, rgb, IntoElement, Pixels};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnlineStatus {
    Online,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Leader,
    Member,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarSize {
    Small,
    Medium,
    Large,
}

impl AvatarSize {
    fn avatar_px(self) -> Pixels {
        match self {
            Self::Small => px(36.0),
            Self::Medium => px(56.0),
            Self::Large => px(80.0),
        }
    }

    fn font_px(self) -> Pixels {
        match self {
            Self::Small => px(14.0),
            Self::Medium => px(22.0),
            Self::Large => px(32.0),
        }
    }

    fn badge_px(self) -> Pixels {
        match self {
            Self::Small => px(14.0),
            Self::Medium => px(18.0),
            Self::Large => px(22.0),
        }
    }

    fn status_dot_px(self) -> Pixels {
        match self {
            Self::Small => px(8.0),
            Self::Medium => px(10.0),
            Self::Large => px(12.0),
        }
    }
}

pub struct AvatarComponent<'a> {
    nickname: &'a str,
    role: Role,
    status: OnlineStatus,
    size: AvatarSize,
    show_name: bool,
    show_status_text: bool,
}

impl<'a> AvatarComponent<'a> {
    pub fn new(nickname: &'a str) -> Self {
        Self {
            nickname,
            role: Role::Member,
            status: OnlineStatus::Offline,
            size: AvatarSize::Medium,
            show_name: true,
            show_status_text: true,
        }
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    pub fn status(mut self, status: OnlineStatus) -> Self {
        self.status = status;
        self
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }

    pub fn show_name(mut self, v: bool) -> Self {
        self.show_name = v;
        self
    }

    pub fn show_status_text(mut self, v: bool) -> Self {
        self.show_status_text = v;
        self
    }

    pub fn render(self, theme: &Theme) -> impl IntoElement {
        let avatar_size = self.size.avatar_px();
        let font_size = self.size.font_px();
        let badge_size = self.size.badge_px();
        let dot_size = self.size.status_dot_px();
        let first_char = self.nickname.chars().next().unwrap_or('?').to_string();

        let is_online = self.status == OnlineStatus::Online;
        let is_leader = self.role == Role::Leader;

        let border_color = if is_online {
            theme.colors.success
        } else {
            theme.colors.border
        };

        let bg_color = avatar_bg_color(self.nickname);

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(theme.spacing.xs)
            .child(
                div()
                    .relative()
                    // avatar circle with rounded border
                    .child(
                        div()
                            .w(avatar_size)
                            .h(avatar_size)
                            .bg(rgb(bg_color))
                            .rounded(theme.radius.full)
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_2()
                            .border_color(rgb(border_color))
                            .child(
                                div()
                                    .text_size(font_size)
                                    .text_color(rgb(0xffffff))
                                    .child(first_char),
                            ),
                    )
                    // online status dot (bottom-right of avatar)
                    .child(
                        div()
                            .absolute()
                            .bottom(px(0.0))
                            .right(px(0.0))
                            .w(dot_size)
                            .h(dot_size)
                            .rounded(theme.radius.full)
                            .bg(rgb(if is_online {
                                theme.colors.success
                            } else {
                                theme.colors.text_disabled
                            }))
                            .border_2()
                            .border_color(rgb(theme.colors.surface)),
                    )
                    // leader crown badge (top-right)
                    .when(is_leader, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top(px(-2.0))
                                .right(px(-4.0))
                                .w(badge_size)
                                .h(badge_size)
                                .bg(rgb(theme.colors.warning))
                                .rounded(theme.radius.full)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .text_size(badge_size * 0.6)
                                        .child("👑"),
                                ),
                        )
                    }),
            )
            // nickname
            .when(self.show_name, |this| {
                this.child(
                    div()
                        .text_size(theme.typography.caption.size)
                        .text_color(rgb(theme.colors.text))
                        .max_w(avatar_size + px(20.0))
                        .overflow_x_hidden()
                        .text_ellipsis()
                        .child(self.nickname.to_string()),
                )
            })
            // role label
            .when(self.show_name && is_leader, |this| {
                this.child(
                    div()
                        .px(theme.spacing.xs)
                        .py(px(1.0))
                        .bg(rgb(theme.colors.warning))
                        .rounded(theme.radius.sm)
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(theme.colors.background))
                                .child("Leader"),
                        ),
                )
            })
            // online/offline text
            .when(self.show_status_text, |this| {
                this.child(
                    div()
                        .text_size(px(10.0))
                        .text_color(rgb(if is_online {
                            theme.colors.success
                        } else {
                            theme.colors.text_disabled
                        }))
                        .child(if is_online { "在线" } else { "离线" }),
                )
            })
    }
}

pub fn avatar_bg_color(nickname: &str) -> u32 {
    const PALETTE: [u32; 8] = [
        0x89b4fa, // blue
        0xa6e3a1, // green
        0xf9e2af, // yellow
        0xf38ba8, // pink
        0xcba6f7, // mauve
        0xfab387, // peach
        0x94e2d5, // teal
        0xf5c2e7, // flamingo
    ];
    let hash: u32 = nickname.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    PALETTE[(hash as usize) % PALETTE.len()]
}
