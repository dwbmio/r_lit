//! Launch subcommand — GPUI View layer.
//!
//! This module is the View in MVC. It only does:
//! - Rendering UI from model state
//! - Dispatching user events to controller functions
//! - Polling async results and applying them to the model

use crate::config::Config;
use crate::controller;
use crate::error::Result;
use crate::gui::components::avatar::AvatarSize;
use crate::gui::components::AvatarComponent;
use crate::gui::pages::GroupDiscoveryPage;
use crate::gui::popviews::LoadingPopView;
use crate::gui::{Theme, Toast};
use crate::model::*;
use crate::user_db::{UserDatabase, UserInfo};
use gpui::{
    Application, Bounds, Context, Entity, Render, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};
use gpui_component::input::{Input, InputState};
use gpui_component::Root;

// ── Entry Point ─────────────────────────────────────────────────────

pub fn run(width: u32, height: u32, nickname: Option<String>) -> Result<()> {
    log::info!("Launching workbench with dimensions: {}x{}", width, height);

    let config = Config::load()?;
    config.ensure_dirs()?;

    log::info!("Using data directory: {:?}", config.data_dir);

    let (has_user, current_user) = {
        let db_path = config.user_db_path();
        let user_db = UserDatabase::open(db_path)?;

        let result = if let Some(nick) = nickname {
            let user = UserInfo::new(nick);
            let _ = user_db.save_user(&user);
            (true, user)
        } else if user_db.has_user() {
            (true, user_db.get_current_user()?.expect("User should exist"))
        } else {
            (false, UserInfo::new("临时用户".to_string()))
        };
        // user_db dropped here, releasing the lock
        result
    };

    log::info!("Current user: {} ({})", current_user.nickname, current_user.id);

    Application::new().run(move |cx| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
        let config_clone = config.clone();
        let has_user_clone = has_user;
        let current_user_clone = current_user.clone();

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Group Vibe Workbench".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| WorkbenchView::new(
                    current_user_clone,
                    !has_user_clone,
                    config_clone,
                    window,
                    cx,
                ));
                let any_view: gpui::AnyView = view.into();
                cx.new(|cx| Root::new(any_view, window, cx))
            },
        )
        .expect("Failed to open window");

        cx.activate(true);
    });

    Ok(())
}

// ── View ────────────────────────────────────────────────────────────

struct WorkbenchView {
    model: AppModel,
    config: Config,
    nickname_input: Entity<InputState>,
    group_id_input: Entity<InputState>,
    group_discovery: Option<GroupDiscoveryPage>,
    loading_popview: Option<LoadingPopView>,
}

impl WorkbenchView {
    fn new(
        current_user: UserInfo,
        show_login: bool,
        config: Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let nickname_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("输入你的昵称 (2-20 字符)")
        });

        let group_id_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("输入群组 ID 或留空自动生成")
        });

        let group_discovery = if !show_login {
            Some(GroupDiscoveryPage::new(current_user.clone(), group_id_input.clone()))
        } else {
            None
        };

        Self {
            model: AppModel::new(current_user, show_login),
            config,
            nickname_input,
            group_id_input,
            group_discovery,
            loading_popview: None,
        }
    }

    // ── Event handlers (thin wrappers that call controller) ─────────

    fn on_use_default_nickname(&mut self, cx: &mut Context<Self>) {
        controller::use_default_nickname(&mut self.model, &self.config);
        cx.notify();
    }

    fn on_use_custom_nickname(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.nickname_input.read(cx).text().to_string();
        let trimmed = text.trim();

        if let Err(msg) = controller::validate_nickname(trimmed) {
            self.model.push_toast(Toast::warning(msg));
            if trimmed.is_empty() {
                self.nickname_input.update(cx, |state, cx| {
                    state.focus(window, cx);
                });
            }
            cx.notify();
            return;
        }

        let user = UserInfo::new(trimmed.to_string());
        controller::handle_login(&mut self.model, user, &self.config);
        cx.notify();
    }

    fn on_start_search(&mut self, cx: &mut Context<Self>) {
        let rx = controller::start_search(&mut self.model, &self.config);
        self.poll_result(rx, |view, _cx, result| {
            controller::apply_search_result(&mut view.model, result);
        }, cx);
        cx.notify();
    }

    fn on_cancel_search(&mut self, cx: &mut Context<Self>) {
        controller::cancel_search(&mut self.model);
        cx.notify();
    }

    fn on_create_new_group(&mut self, cx: &mut Context<Self>) {
        let group_id = controller::create_new_group(&mut self.model);
        self.do_join_group(group_id, cx);
    }

    fn on_join_group_from_input(&mut self, cx: &mut Context<Self>) {
        let text = self.group_id_input.read(cx).text().to_string();
        let trimmed = text.trim().to_string();

        if trimmed.is_empty() {
            self.on_create_new_group(cx);
        } else {
            self.model.push_toast(Toast::success(format!("正在加入群组: {}", trimmed)));
            self.do_join_group(trimmed, cx);
        }
    }

    fn on_join_discovered_group(&mut self, group_id: String, cx: &mut Context<Self>) {
        self.do_join_group(group_id, cx);
    }

    fn do_join_group(&mut self, group_id: String, cx: &mut Context<Self>) {
        let rx = controller::join_group(&mut self.model, group_id, &self.config);
        self.poll_result(rx, |view, cx, result| {
            controller::apply_join_result(&mut view.model, result);
            let member_rx = controller::start_member_poll();
            view.start_member_polling(member_rx, cx);
        }, cx);
        cx.notify();
    }

    fn on_leave_group(&mut self, cx: &mut Context<Self>) {
        controller::leave_group(&mut self.model);
        cx.notify();
    }

    fn on_start_collaboration(&mut self, cx: &mut Context<Self>) {
        controller::start_collaboration(&mut self.model, &self.config);
        cx.notify();
    }

    // ── Async result polling helpers ────────────────────────────────

    /// One-shot: wait for a single result from `rx`, then apply.
    fn poll_result<T, F>(
        &self,
        rx: std::sync::mpsc::Receiver<T>,
        apply: F,
        cx: &mut Context<Self>,
    )
    where
        T: Send + 'static,
        F: FnOnce(&mut WorkbenchView, &mut Context<WorkbenchView>, T) + Send + 'static,
    {
        cx.spawn(async move |weak: gpui::WeakEntity<WorkbenchView>, app: &mut gpui::AsyncApp| {
            loop {
                gpui::Timer::after(std::time::Duration::from_millis(500)).await;
                match rx.try_recv() {
                    Ok(result) => {
                        let _ = app.update(|cx| {
                            if let Some(entity) = weak.upgrade() {
                                entity.update(cx, |view, cx| {
                                    apply(view, cx, result);
                                    cx.notify();
                                });
                            }
                        });
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        })
        .detach();
    }

    /// Continuous: keep reading from `rx` and apply each update until channel closes.
    fn poll_stream<T, F>(
        &self,
        rx: std::sync::mpsc::Receiver<T>,
        apply: F,
        cx: &mut Context<Self>,
    )
    where
        T: Send + 'static,
        F: Fn(&mut WorkbenchView, T) + Send + 'static,
    {
        cx.spawn(async move |weak: gpui::WeakEntity<WorkbenchView>, app: &mut gpui::AsyncApp| {
            loop {
                gpui::Timer::after(std::time::Duration::from_millis(500)).await;
                match rx.try_recv() {
                    Ok(result) => {
                        let should_stop = app.update(|cx| {
                            if let Some(entity) = weak.upgrade() {
                                entity.update(cx, |view, cx| {
                                    apply(view, result);
                                    cx.notify();
                                });
                                false
                            } else {
                                true
                            }
                        }).unwrap_or(true);
                        if should_stop { break; }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        })
        .detach();
    }

    fn start_member_polling(
        &self,
        rx: std::sync::mpsc::Receiver<MemberUpdate>,
        cx: &mut Context<Self>,
    ) {
        self.poll_stream(rx, |view, update| {
            controller::apply_member_update(&mut view.model, update);
        }, cx);
    }
}

// ── Render ───────────────────────────────────────────────────────────

impl Render for WorkbenchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::default();

        if self.model.should_close_loading {
            self.loading_popview = None;
            self.model.should_close_loading = false;
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme.colors.background))
            .when(self.model.state == AppState::Login, |this| {
                this.child(self.render_login(&theme, cx))
            })
            .when(self.model.state == AppState::GroupDiscovery, |this| {
                this.child(self.render_group_discovery(&theme, cx))
            })
            .when(self.model.state == AppState::Searching, |this| {
                this.child(self.render_searching(&theme, cx))
            })
            .when(self.model.state == AppState::GroupLobby, |this| {
                this.child(self.render_group_lobby(&theme, cx))
            })
            .child(self.model.toast_queue.render(&theme))
    }
}

// ── Render Helpers (pure view, read-only model access) ──────────────

impl WorkbenchView {
    fn render_login(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div().absolute().size_full()
                    .bg(rgb(theme.colors.mask)).opacity(theme.colors.mask_opacity)
            )
            .child(
                div()
                    .w(px(500.0))
                    .bg(rgb(theme.colors.surface))
                    .rounded(theme.radius.lg)
                    .p(theme.spacing.xl)
                    .flex().flex_col().gap(theme.spacing.lg)
                    .child(
                        div().text_size(theme.typography.heading.size)
                            .text_color(rgb(theme.colors.text)).child("欢迎使用")
                    )
                    .child(
                        div().text_size(theme.typography.body.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("请输入你的昵称开始使用")
                    )
                    .child(
                        div().w_full().child(Input::new(&self.nickname_input).cleanable(true))
                    )
                    .child(
                        div().flex().gap(theme.spacing.md)
                            .child(
                                div().flex_1()
                                    .px(theme.spacing.xl).py(theme.spacing.md)
                                    .bg(rgb(theme.colors.primary)).rounded(theme.radius.md)
                                    .cursor_pointer().text_center()
                                    .hover(|s| s.bg(rgb(0x74c7ec)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.on_use_default_nickname(cx);
                                    }))
                                    .child(div().text_size(theme.typography.button.size)
                                        .text_color(rgb(0xffffff)).child("使用随机昵称"))
                            )
                            .child(
                                div().flex_1()
                                    .px(theme.spacing.xl).py(theme.spacing.md)
                                    .bg(rgb(theme.colors.success)).rounded(theme.radius.md)
                                    .cursor_pointer().text_center()
                                    .hover(|s| s.bg(rgb(0x94e2d5)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, window, cx| {
                                        this.on_use_custom_nickname(window, cx);
                                    }))
                                    .child(div().text_size(theme.typography.button.size)
                                        .text_color(rgb(0xffffff)).child("开始使用"))
                            )
                    )
            )
    }

    fn render_group_discovery(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex().flex_col().items_center().justify_center().size_full()
            .child(
                div()
                    .w(px(480.0))
                    .bg(rgb(theme.colors.surface)).rounded(theme.radius.lg)
                    .p(theme.spacing.xl)
                    .flex().flex_col().gap(theme.spacing.lg)
                    .child(
                        div().text_size(theme.typography.heading.size)
                            .text_color(rgb(theme.colors.text))
                            .child(format!("欢迎, {}!", self.model.current_user.nickname))
                    )
                    .child(
                        div().text_size(theme.typography.body.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("输入群组 ID 加入现有群组，或创建新群组")
                    )
                    .child(
                        div().flex().flex_col().gap(theme.spacing.sm)
                            .child(div().text_size(theme.typography.body.size)
                                .text_color(rgb(theme.colors.text)).child("群组 ID:"))
                            .child(div().w_full().child(Input::new(&self.group_id_input).cleanable(true)))
                    )
                    .child(
                        div().flex().gap(theme.spacing.md)
                            .child(
                                div().flex_1()
                                    .px(theme.spacing.xl).py(theme.spacing.md)
                                    .bg(rgb(theme.colors.primary)).rounded(theme.radius.md)
                                    .cursor_pointer().text_center()
                                    .hover(|s| s.bg(rgb(0x74c7ec)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.on_join_group_from_input(cx);
                                    }))
                                    .child(div().text_size(theme.typography.button.size)
                                        .text_color(rgb(0xffffff)).child("加入群组"))
                            )
                            .child(
                                div().flex_1()
                                    .px(theme.spacing.xl).py(theme.spacing.md)
                                    .bg(rgb(theme.colors.surface_variant)).rounded(theme.radius.md)
                                    .cursor_pointer().text_center()
                                    .hover(|s| s.bg(rgb(theme.colors.border)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.on_create_new_group(cx);
                                    }))
                                    .child(div().text_size(theme.typography.button.size)
                                        .text_color(rgb(theme.colors.text)).child("创建新群组"))
                            )
                    )
                    .child(
                        div().w_full()
                            .px(theme.spacing.xl).py(theme.spacing.md)
                            .bg(rgb(theme.colors.info)).rounded(theme.radius.md)
                            .cursor_pointer().text_center()
                            .hover(|s| s.bg(rgb(0x74c7ec)))
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.on_start_search(cx);
                            }))
                            .child(div().text_size(theme.typography.button.size)
                                .text_color(rgb(0xffffff)).child("搜索本地网络上的群组"))
                    )
                    .child(
                        div().text_size(theme.typography.caption.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("提示: 同一局域网的成员会自动发现并连接")
                    )
            )
    }

    fn render_searching(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let is_loading = self.model.swarm_state == SwarmState::Connecting;
        let has_results = !self.model.discovered_groups.is_empty();
        let has_error = self.model.search_error.is_some();

        div()
            .flex().flex_col().items_center().justify_center().size_full()
            .child(
                div()
                    .w(px(520.0))
                    .bg(rgb(theme.colors.surface)).rounded(theme.radius.lg)
                    .p(theme.spacing.xl)
                    .flex().flex_col().gap(theme.spacing.lg)
                    .child(
                        div().flex().items_center().justify_between()
                            .child(
                                div().text_size(theme.typography.heading.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child(if is_loading { "正在搜索..." } else { "搜索结果" })
                            )
                            .child(
                                div().px(theme.spacing.md).py(px(6.0))
                                    .bg(rgb(theme.colors.surface_variant)).rounded(theme.radius.sm)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(theme.colors.border)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.on_cancel_search(cx);
                                    }))
                                    .child(div().text_size(theme.typography.caption.size)
                                        .text_color(rgb(theme.colors.text)).child("返回"))
                            )
                    )
                    .when(is_loading, |this| {
                        this.child(
                            div().flex().flex_col().items_center().gap(theme.spacing.md).py(theme.spacing.xl)
                                .child(div().text_size(px(48.0)).child("🔍"))
                                .child(div().text_size(theme.typography.body.size)
                                    .text_color(rgb(theme.colors.text_secondary))
                                    .child("正在搜索本地网络，预计 10-12 秒..."))
                                .child(
                                    div().w(px(300.0)).h(px(4.0))
                                        .bg(rgb(theme.colors.surface_variant)).rounded(px(2.0))
                                        .overflow_hidden()
                                        .child(div().h_full().w(px(120.0))
                                            .bg(rgb(theme.colors.info)).rounded(px(2.0)))
                                )
                        )
                    })
                    .when(has_error && !is_loading, |this| {
                        let msg = self.model.search_error.clone().unwrap_or_default();
                        this.child(
                            div().flex().flex_col().items_center().gap(theme.spacing.md).py(theme.spacing.lg)
                                .child(div().text_size(px(48.0)).child("😔"))
                                .child(div().text_size(theme.typography.body.size)
                                    .text_color(rgb(theme.colors.error)).text_center().child(msg))
                        )
                    })
                    .when(has_results && !is_loading, |this| {
                        this.children(
                            self.model.discovered_groups.iter().map(|group| {
                                let names: Vec<String> = group.members.iter()
                                    .map(|(_, n)| n.clone()).collect();
                                let member_str = names.join(", ");
                                let group_id = group.group_id.clone();

                                div().flex().items_center().justify_between().w_full()
                                    .px(theme.spacing.md).py(theme.spacing.md)
                                    .bg(rgb(theme.colors.background)).rounded(theme.radius.md)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(theme.colors.surface_variant)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _, cx| {
                                        this.on_join_discovered_group(group_id.clone(), cx);
                                    }))
                                    .child(
                                        div().flex().flex_col().gap(px(4.0))
                                            .child(div().text_size(theme.typography.body.size)
                                                .text_color(rgb(theme.colors.text))
                                                .child(format!("群组: {}", group.group_id)))
                                            .child(div().text_size(theme.typography.caption.size)
                                                .text_color(rgb(theme.colors.text_secondary))
                                                .child(format!("{} 人 · {}", group.members.len(), member_str)))
                                    )
                                    .child(
                                        div().text_size(theme.typography.body.size)
                                            .text_color(rgb(theme.colors.primary)).child("加入 →")
                                    )
                            })
                        )
                    })
                    .when(!has_results && !has_error && !is_loading, |this| {
                        this.child(
                            div().flex().flex_col().items_center().gap(theme.spacing.md).py(theme.spacing.lg)
                                .child(div().text_size(px(48.0)).child("📡"))
                                .child(div().text_size(theme.typography.body.size)
                                    .text_color(rgb(theme.colors.text_secondary))
                                    .child("未发现可用群组"))
                        )
                    })
            )
    }

    fn render_group_lobby(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(ref lobby) = self.model.group_lobby {
            div().flex().flex_col().size_full()
                // Header bar
                .child(
                    div().flex().items_center().justify_between()
                        .h(px(56.0)).px(theme.spacing.xl)
                        .bg(rgb(theme.colors.surface))
                        .border_b_1().border_color(rgb(theme.colors.border))
                        .child(
                            div().flex().items_center().gap(theme.spacing.md)
                                .child(div().text_size(theme.typography.subheading.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child(format!("群组: {}", lobby.group_id)))
                                .child(self.render_swarm_badge(theme))
                                .child(
                                    div().px(theme.spacing.sm).py(px(4.0))
                                        .bg(rgb(theme.colors.surface_variant)).rounded(theme.radius.sm)
                                        .child(div().text_size(theme.typography.caption.size)
                                            .text_color(rgb(theme.colors.text_secondary))
                                            .child(format!("{}/{} 在线",
                                                lobby.online_count(), lobby.total_count())))
                                )
                        )
                        .child(
                            div().flex().items_center().gap(theme.spacing.sm)
                                .child(
                                    div().px(theme.spacing.md).py(px(6.0))
                                        .bg(rgb(theme.colors.primary)).rounded(theme.radius.sm)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(0x74c7ec)))
                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                            this.on_start_collaboration(cx);
                                        }))
                                        .child(div().text_size(theme.typography.caption.size)
                                            .text_color(rgb(0xffffff)).child("开始协作"))
                                )
                                .child(
                                    div().px(theme.spacing.md).py(px(6.0))
                                        .bg(rgb(theme.colors.error)).rounded(theme.radius.sm)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(rgb(0xf38ba8)))
                                        .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                            this.on_leave_group(cx);
                                        }))
                                        .child(div().text_size(theme.typography.caption.size)
                                            .text_color(rgb(0xffffff)).child("退出"))
                                )
                        )
                )
                // Main content area
                .child(
                    div().flex_1().relative()
                        .child(
                            if lobby.others.is_empty() {
                                // empty state
                                div().flex().flex_col().items_center().justify_center()
                                    .size_full().gap(theme.spacing.md)
                                    .child(div().text_size(px(64.0)).child("👥"))
                                    .child(div().text_size(theme.typography.subheading.size)
                                        .text_color(rgb(theme.colors.text_secondary))
                                        .child("等待其他成员加入..."))
                                    .child(div().text_size(theme.typography.caption.size)
                                        .text_color(rgb(theme.colors.text_disabled))
                                        .child("同一局域网的成员会自动发现并连接"))
                            } else {
                                // other members: top area, left to right
                                div().flex().flex_col().size_full()
                                    .p(theme.spacing.xl)
                                    .child(
                                        div().text_size(theme.typography.caption.size)
                                            .text_color(rgb(theme.colors.text_secondary))
                                            .mb(theme.spacing.md)
                                            .child("成员")
                                    )
                                    .child(
                                        div().flex().flex_wrap().gap(theme.spacing.lg)
                                            .children(lobby.others.iter().map(|m| {
                                                Self::render_avatar(m, AvatarSize::Medium, theme)
                                            }))
                                    )
                            }
                        )
                )
                // Bottom bar: my info on the left
                .child(
                    div().flex().items_center().justify_between()
                        .h(px(56.0)).px(theme.spacing.xl)
                        .bg(rgb(theme.colors.surface))
                        .border_t_1().border_color(rgb(theme.colors.border))
                        .child(
                            Self::render_self_bar(&lobby.me, theme)
                        )
                        .child(
                            div().text_size(theme.typography.caption.size)
                                .text_color(rgb(theme.colors.text_disabled))
                                .child(format!("群组 ID: {}", lobby.group_id))
                        )
                )
        } else {
            div().child("未加入群组")
        }
    }

    fn render_swarm_badge(&self, theme: &Theme) -> impl IntoElement {
        let (color, text) = match self.model.swarm_state {
            SwarmState::Connected => (theme.colors.success, "已连接"),
            SwarmState::Connecting => (theme.colors.warning, "连接中..."),
            SwarmState::Error => (theme.colors.error, "连接失败"),
            SwarmState::Disconnected => (theme.colors.text_disabled, "未连接"),
        };

        div().px(theme.spacing.sm).py(px(4.0))
            .bg(rgb(color)).rounded(theme.radius.sm)
            .child(div().text_size(theme.typography.caption.size)
                .text_color(rgb(0xffffff)).child(text))
    }

    /// Bottom bar: horizontal self info (avatar + name + role badge + status dot)
    fn render_self_bar(me: &AvatarModel, theme: &Theme) -> impl IntoElement {
        let first_char = me.nickname.chars().next().unwrap_or('?').to_string();
        let bg = crate::gui::components::avatar::avatar_bg_color(&me.nickname);

        div().flex().items_center().gap(theme.spacing.sm)
            // small avatar circle
            .child(
                div().relative()
                    .child(
                        div().w(px(32.0)).h(px(32.0))
                            .bg(rgb(bg))
                            .rounded(theme.radius.full)
                            .flex().items_center().justify_center()
                            .border_1().border_color(rgb(theme.colors.success))
                            .child(div().text_size(px(14.0)).text_color(rgb(0xffffff)).child(first_char))
                    )
                    .child(
                        div().absolute().bottom(px(-1.0)).right(px(-1.0))
                            .w(px(8.0)).h(px(8.0))
                            .rounded(theme.radius.full)
                            .bg(rgb(if me.is_online() { theme.colors.success } else { theme.colors.text_disabled }))
                            .border_1().border_color(rgb(theme.colors.surface))
                    )
            )
            // name
            .child(
                div().text_size(theme.typography.body.size)
                    .text_color(rgb(theme.colors.text))
                    .child(me.nickname.clone())
            )
            // role badge
            .when(me.is_leader(), |this| {
                this.child(
                    div().px(theme.spacing.xs).py(px(1.0))
                        .bg(rgb(theme.colors.warning)).rounded(theme.radius.sm)
                        .child(div().text_size(px(10.0))
                            .text_color(rgb(theme.colors.background)).child("Leader"))
                )
            })
    }

    fn render_avatar(avatar: &AvatarModel, size: AvatarSize, theme: &Theme) -> impl IntoElement {
        use crate::gui::components::avatar::{
            OnlineStatus as ViewOnlineStatus,
            Role as ViewRole,
        };

        let role = if avatar.is_leader() { ViewRole::Leader } else { ViewRole::Member };
        let status = if avatar.is_online() { ViewOnlineStatus::Online } else { ViewOnlineStatus::Offline };

        AvatarComponent::new(&avatar.nickname)
            .role(role)
            .status(status)
            .size(size)
            .render(theme)
    }
}
