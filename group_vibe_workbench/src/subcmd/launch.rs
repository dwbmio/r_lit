use crate::error::Result;
use crate::config::Config;
use crate::user_db::{UserDatabase, UserInfo};
use crate::gui::pages::{LoginPopView, GroupDiscoveryPage, GroupLobbyPage, GroupMember};
use crate::gui::popviews::LoadingPopView;
use crate::gui::{Theme, Toast, ToastQueue};
use crate::shared_file::SharedFile;
use gpui::{
    Application, Bounds, Context, Render, Window, WindowBounds, WindowOptions, div, prelude::*,
    px, rgb, size,
};
use std::path::PathBuf;
use murmur::Swarm;

pub fn run(width: u32, height: u32, nickname: Option<String>) -> Result<()> {
    log::info!("Launching workbench with dimensions: {}x{}", width, height);

    // Load configuration
    let config = Config::load()?;
    config.ensure_dirs()?;

    log::info!("Using data directory: {:?}", config.data_dir);

    // 初始化用户数据库
    let db_path = config.user_db_path();
    let user_db = UserDatabase::open(db_path)?;

    // 检查是否有用户信息或提供了昵称
    let (has_user, current_user) = if let Some(nick) = nickname {
        log::info!("Using provided nickname: {}", nick);
        let user = UserInfo::new(nick);
        // 保存到数据库
        let _ = user_db.save_user(&user);
        (true, user)
    } else if user_db.has_user() {
        (true, user_db.get_current_user()?.expect("User should exist"))
    } else {
        (false, UserInfo::new("临时用户".to_string()))
    };

    log::info!("Current user: {} ({})", current_user.nickname, current_user.id);

    Application::new().run(move |cx| {
        // Initialize gpui-component
        gpui_component::init(cx);

        // Calculate window bounds
        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);

        // Clone config for the closure
        let config_clone = config.clone();

        // Open main window
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
            |_, cx| {
                cx.new(|_| WorkbenchView::new(current_user.clone(), !has_user, config_clone))
            },
        )
        .expect("Failed to open window");

        // Activate the application
        cx.activate(true);
    });

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppState {
    Login,
    GroupDiscovery,
    GroupLobby,
}

struct WorkbenchView {
    current_user: UserInfo,
    state: AppState,
    group_discovery: Option<GroupDiscoveryPage>,
    group_lobby: Option<GroupLobbyPage>,
    toast_queue: ToastQueue,
    swarm: Option<Swarm>,
    nickname_input: String,
    loading_popview: Option<LoadingPopView>,
    shared_file: Option<std::sync::Arc<SharedFile>>,
    config: Config,
    should_close_loading: bool,
}

impl WorkbenchView {
    fn new(current_user: UserInfo, show_login: bool, config: Config) -> Self {
        let state = if show_login {
            AppState::Login
        } else {
            AppState::GroupDiscovery
        };

        let group_discovery = if !show_login {
            Some(GroupDiscoveryPage::new(current_user.clone()))
        } else {
            None
        };

        Self {
            current_user,
            state,
            group_discovery,
            group_lobby: None,
            toast_queue: ToastQueue::new(),
            swarm: None,
            discovery_handle: None,
            nickname_input: String::new(),
            loading_popview: None,
            shared_file: None,
            config,
            should_close_loading: false,
        }
    }

    fn handle_login(&mut self, user: UserInfo) {
        log::info!("User logged in: {} ({})", user.nickname, user.id);

        // 保存用户信息到数据库
        let db_path = self.config.user_db_path();
        if let Ok(user_db) = UserDatabase::open(db_path) {
            if let Err(e) = user_db.save_user(&user) {
                log::error!("Failed to save user: {:?}", e);
                self.toast_queue.push(Toast::error("保存用户信息失败"));
            } else {
                self.toast_queue.push(Toast::success(format!("欢迎, {}!", user.nickname)));
            }
        }

        self.current_user = user;
        self.state = AppState::GroupDiscovery;
        self.group_discovery = Some(GroupDiscoveryPage::new(self.current_user.clone()));
    }

    fn use_default_nickname(&mut self) {
        let nickname = format!("User_{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
        let user = UserInfo::new(nickname);
        self.handle_login(user);
    }

    fn use_custom_nickname(&mut self) {
        if self.nickname_input.trim().is_empty() {
            self.toast_queue.push(Toast::warning("请输入昵称".to_string()));
            return;
        }
        let user = UserInfo::new(self.nickname_input.trim().to_string());
        self.handle_login(user);
    }

    fn start_discovery(&mut self, _cx: &mut Context<Self>) {
        log::info!("Group discovery is now automatic with LocalSwarmDiscovery");
        log::info!("Users should directly create or join a group by ID");

        // 显示提示
        self.toast_queue.push(Toast::info("输入群组 ID 或创建新群组开始协作".to_string()));
    }

    fn create_new_group(&mut self, cx: &mut Context<Self>) {
        log::info!("Creating new group...");

        // 生成一个随机群组 ID
        let group_id = format!("group_{}", chrono::Utc::now().timestamp());

        self.toast_queue.push(Toast::success(format!("创建群组: {}", group_id)));
        self.join_group(group_id, cx);
    }

    fn join_group(&mut self, group_id: String, _cx: &mut Context<Self>) {
        log::info!("Joining group: {}", group_id);

        self.toast_queue.push(Toast::success(format!("正在加入群组: {}", group_id)));

        // 先切换到群组大厅
        let mut lobby = GroupLobbyPage::new(self.current_user.clone(), group_id.clone());

        // 添加当前用户
        lobby.add_member(GroupMember {
            id: self.current_user.id.clone(),
            nickname: self.current_user.nickname.clone(),
            avatar: None,
            is_online: true,
            is_leader: true,
        });

        self.group_lobby = Some(lobby);
        self.state = AppState::GroupLobby;

        // 在后台初始化 Swarm 并连接
        let user = self.current_user.clone();
        let group_id_clone = group_id.clone();
        let swarm_path = self.config.swarm_path(&user.id);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                // 使用全局 Swarm 管理器
                let swarm_config = crate::swarm_manager::SwarmConfig {
                    storage_path: swarm_path,
                    group_id: group_id_clone.clone(),
                };

                match crate::swarm_manager::get_or_init_swarm(swarm_config).await {
                    Ok(swarm) => {
                        log::info!("✅ Swarm initialized for group: {}", group_id_clone);

                        // 等待 LocalSwarmDiscovery 发现其他节点
                        log::info!("⏳ Waiting for peer discovery (10 seconds)...");
                        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

                        // 连接到发现的节点
                        log::info!("🔍 Connecting to discovered peers...");
                        match swarm.discover_and_connect_local_peers().await {
                            Ok(count) => {
                                if count > 0 {
                                    log::info!("✅ Connected to {} peer(s)", count);

                                    // 获取连接的节点列表
                                    let peers = swarm.connected_peers().await;
                                    for peer_id in peers {
                                        log::info!("  - Connected peer: {}", peer_id);
                                    }
                                } else {
                                    log::info!("⚠️  No peers found yet. Will continue listening in background.");
                                }
                            }
                            Err(e) => {
                                log::error!("❌ Failed to connect to peers: {:?}", e);
                            }
                        }

                        // 保持 Swarm 运行
                        // 在实际应用中，Swarm 应该一直运行直到用户退出群组
                        log::info!("✅ Swarm is now running. Peers will auto-discover and connect.");
                    }
                    Err(e) => {
                        log::error!("❌ Failed to initialize swarm: {:?}", e);
                    }
                }
            });
        });
    }

    fn leave_group(&mut self) {
        log::info!("Leaving group");

        self.toast_queue.push(Toast::info("已退出群组".to_string()));

        // 清理群组状态
        self.group_lobby = None;
        self.swarm = None;

        // 在后台关闭全局 Swarm
        std::thread::spawn(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                if let Err(e) = crate::swarm_manager::shutdown_swarm().await {
                    log::error!("Failed to shutdown swarm: {:?}", e);
                } else {
                    log::info!("✅ Global Swarm shutdown successfully");
                }
            });
        });

        // 返回到群组发现页面
        self.state = AppState::GroupDiscovery;
        if self.group_discovery.is_none() {
            self.group_discovery = Some(GroupDiscoveryPage::new(self.current_user.clone()));
        }
    }

    fn close_loading(&mut self) {
        self.loading_popview = None;
    }

    fn start_collaboration(&mut self) {
        log::info!("Starting collaboration");

        if let Some(ref lobby) = self.group_lobby {
            let group_id = lobby.group_id.clone();
            let member_count = lobby.members.len();

            // 创建共享文件路径
            let shared_file_path = self.config.shared_file_path.clone();

            // 如果文件不存在，创建它
            if !shared_file_path.exists() {
                if let Err(e) = std::fs::write(&shared_file_path, format!(
                    "# 群组协作文件: {}\n\n欢迎来到协作空间！\n\n使用你喜欢的编辑器编辑此文件，所有更改会自动同步到群组成员。\n\n",
                    group_id
                )) {
                    log::error!("Failed to create shared file: {:?}", e);
                    self.toast_queue.push(Toast::error("创建共享文件失败".to_string()));
                    return;
                }
            }

            self.toast_queue.push(Toast::success(format!(
                "开始协作！共享文件: chat.ctx"
            )));

            // 初始化 SharedFile 并启动同步
            let user = self.current_user.clone();
            let file_path = shared_file_path.clone();
            let group_id_for_thread = group_id.clone();

            let shared_file_handle = std::sync::Arc::new(tokio::sync::Mutex::new(None::<std::sync::Arc<SharedFile>>));
            let shared_file_clone = shared_file_handle.clone();

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async move {
                    // 获取全局 Swarm 实例
                    match crate::swarm_manager::get_swarm().await {
                        Some(swarm) => {
                            log::info!("✅ Using global Swarm for SharedFile");

                            match SharedFile::new(
                                swarm,
                                "chat_ctx".to_string(),
                                file_path.clone(),
                            ).await {
                                Ok(shared_file) => {
                                    log::info!("✅ SharedFile initialized for group: {}", group_id_for_thread);

                                    let shared_file = std::sync::Arc::new(shared_file);

                                    // 启动文件监听
                                    if let Err(e) = shared_file.start_watching().await {
                                        log::error!("❌ Failed to start file watching: {:?}", e);
                                    } else {
                                        log::info!("✅ File watching started");
                                    }

                                    // 保存 shared_file 引用
                                    *shared_file_clone.lock().await = Some(shared_file.clone());

                                    // 获取初始内容
                                    let content = shared_file.get_content().await;
                                    log::info!("📄 Initial content length: {} bytes", content.len());
                                }
                                Err(e) => {
                                    log::error!("❌ Failed to initialize SharedFile: {:?}", e);
                                }
                            }
                        }
                        None => {
                            log::error!("❌ No global Swarm instance found! Please join a group first.");
                        }
                    }
                });
            });

            // 保存 shared_file 引用到 self（需要异步获取）
            // 注意：这里我们无法直接保存，因为在 spawn 中
            // 实际应用中可能需要使用消息传递或其他机制

            log::info!("Collaboration started for group: {}", group_id);
        } else {
            self.toast_queue.push(Toast::error("无法开始协作：未加入群组".to_string()));
        }
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::default();

        // 检查是否需要关闭 loading
        if self.should_close_loading {
            self.loading_popview = None;
            self.should_close_loading = false;
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(theme.colors.background))
            // 主内容
            .when(self.state == AppState::Login, |this| {
                this.child(self.render_login_overlay(&theme, cx))
            })
            .when(self.state == AppState::GroupDiscovery, |this| {
                this.child(self.render_group_discovery(&theme, cx))
            })
            .when(self.state == AppState::GroupLobby, |this| {
                this.child(self.render_group_lobby(&theme, cx))
            })
            // Loading PopView
            .when_some(self.loading_popview.as_ref(), |this, loading| {
                let closable = loading.is_closable();
                this.child(
                    div()
                        .absolute()
                        .size_full()
                        // 遮罩层
                        .child(
                            div()
                                .absolute()
                                .size_full()
                                .bg(rgb(theme.colors.mask))
                                .opacity(theme.colors.mask_opacity)
                        )
                        // 加载动画内容（内联渲染）
                        .child(
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
                                        .child(
                                            div()
                                                .text_size(px(64.0))
                                                .child("⏳")
                                        )
                                        .child(
                                            div()
                                                .text_size(theme.typography.subheading.size)
                                                .text_color(rgb(theme.colors.text))
                                                .child("正在搜索本地网络")
                                        )
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
                                        .child(
                                            div()
                                                .text_size(theme.typography.caption.size)
                                                .text_color(rgb(theme.colors.text_secondary))
                                                .child("预计需要 5-8 秒...")
                                        )
                                        // 关闭按钮（如果可关闭）
                                        .when(closable, |this| {
                                            this.child(
                                                div()
                                                    .mt(theme.spacing.lg)
                                                    .px(theme.spacing.lg)
                                                    .py(theme.spacing.md)
                                                    .bg(rgb(theme.colors.surface_variant))
                                                    .rounded(theme.radius.md)
                                                    .text_size(theme.typography.body.size)
                                                    .text_color(rgb(theme.colors.text))
                                                    .cursor_pointer()
                                                    .hover(|style| style.bg(rgb(theme.colors.border)))
                                                    .child("✕ 关闭 (点击此处)")
                                            )
                                        })
                                )
                        )
                )
            })
            // Toast 队列
            .child(self.toast_queue.render(&theme))
    }
}

impl WorkbenchView {
    fn render_login_overlay(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            // 遮罩层
            .child(
                div()
                    .absolute()
                    .size_full()
                    .bg(rgb(theme.colors.mask))
                    .opacity(theme.colors.mask_opacity)
            )
            // 登录弹窗
            .child(
                div()
                    .w(px(500.0))
                    .bg(rgb(theme.colors.surface))
                    .rounded(theme.radius.lg)
                    .p(theme.spacing.xl)
                    .flex()
                    .flex_col()
                    .gap(theme.spacing.lg)
                    .child(
                        div()
                            .text_size(theme.typography.heading.size)
                            .text_color(rgb(theme.colors.text))
                            .child("👋 欢迎使用")
                    )
                    .child(
                        div()
                            .text_size(theme.typography.body.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("请输入你的昵称开始使用")
                    )
                    // 输入框（简化版 - 显示当前输入）
                    .child(
                        div()
                            .w_full()
                            .px(theme.spacing.md)
                            .py(theme.spacing.sm)
                            .bg(rgb(theme.colors.background))
                            .border_1()
                            .border_color(rgb(theme.colors.border))
                            .rounded(theme.radius.md)
                            .child(
                                div()
                                    .text_size(theme.typography.body.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child(if self.nickname_input.is_empty() {
                                        "输入昵称...".to_string()
                                    } else {
                                        self.nickname_input.clone()
                                    })
                            )
                    )
                    // 提示文字
                    .child(
                        div()
                            .text_size(theme.typography.caption.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child("💡 提示：由于 GPUI 0.2.2 的限制，请在终端输入昵称并按回车")
                    )
                    // 按钮组
                    .child(
                        div()
                            .flex()
                            .gap(theme.spacing.md)
                            .child(
                                div()
                                    .flex_1()
                                    .px(theme.spacing.xl)
                                    .py(theme.spacing.md)
                                    .bg(rgb(theme.colors.primary))
                                    .rounded(theme.radius.md)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x89b4fa)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.use_default_nickname();
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_size(theme.typography.button.size)
                                            .text_color(rgb(0xffffff))
                                            .text_center()
                                            .child("使用默认昵称")
                                    )
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .px(theme.spacing.xl)
                                    .py(theme.spacing.md)
                                    .bg(rgb(theme.colors.success))
                                    .rounded(theme.radius.md)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0x94e2d5)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.use_custom_nickname();
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_size(theme.typography.button.size)
                                            .text_color(rgb(0xffffff))
                                            .text_center()
                                            .child("开始使用")
                                    )
                            )
                    )
            )
    }

    fn render_group_discovery(&self, _theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(ref discovery) = self.group_discovery {
            // 直接使用 GroupDiscoveryPage 的 render 方法
            // 注意：这里需要克隆 discovery 因为 render 需要 &mut self
            // 在实际实现中，应该重构为更好的架构
            div()
                .flex()
                .size_full()
                .child("Group Discovery Page - TODO: Implement proper rendering")
        } else {
            div().child("No discovery page")
        }
    }

    fn render_group_lobby(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let lobby = self.group_lobby.as_ref().unwrap();

        div()
            .flex()
            .flex_col()
            .size_full()
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
                            .flex()
                            .items_center()
                            .gap(theme.spacing.md)
                            .child(
                                div()
                                    .text_size(theme.typography.heading.size)
                                    .text_color(rgb(theme.colors.text))
                                    .child(format!("🏠 群组: {}", lobby.group_id))
                            )
                            .child(
                                // 退出按钮
                                div()
                                    .px(theme.spacing.md)
                                    .py(px(6.0))
                                    .bg(rgb(theme.colors.error))
                                    .rounded(theme.radius.sm)
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0xf38ba8)))
                                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                        this.leave_group();
                                        cx.notify();
                                    }))
                                    .child(
                                        div()
                                            .text_size(theme.typography.caption.size)
                                            .text_color(rgb(0xffffff))
                                            .child("🚪 退出群组")
                                    )
                            )
                    )
                    .child(
                        div()
                            .text_size(theme.typography.body.size)
                            .text_color(rgb(theme.colors.text_secondary))
                            .child(format!("你好, {}", self.current_user.nickname))
                    )
            )
            // 成员区域
            .child(
                div()
                    .flex_1()
                    .p(theme.spacing.xxl)
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(theme.spacing.xl)
                            .justify_center()
                            .children(
                                lobby.members.iter().map(|member| {
                                    self.render_member_avatar(member, theme)
                                })
                            )
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
                    .child(
                        div()
                            .px(theme.spacing.xl)
                            .py(theme.spacing.md)
                            .bg(rgb(theme.colors.primary))
                            .rounded(theme.radius.md)
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0x89b4fa)))
                            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| {
                                this.start_collaboration();
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .text_size(theme.typography.button.size)
                                    .text_color(rgb(0xffffff))
                                    .child("🚀 开始协作")
                            )
                    )
            )
    }

    fn render_member_avatar(&self, member: &GroupMember, theme: &Theme) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(theme.spacing.sm)
            .child(
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
            .child(
                div()
                    .text_size(theme.typography.body.size)
                    .text_color(rgb(theme.colors.text))
                    .child(member.nickname.clone())
            )
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
            .when(member.is_leader, |this| {
                this.child(
                    div()
                        .text_size(theme.typography.caption.size)
                        .text_color(rgb(theme.colors.warning))
                        .child("👑 Leader")
                )
            })
    }
}
