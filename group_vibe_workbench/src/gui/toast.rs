use gpui::{div, prelude::*, IntoElement, px, rgb};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use crate::gui::theme::Theme;

/// ToastLevel - Toast 消息级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    /// 信息提示（蓝色）
    Info,
    /// 成功提示（绿色）
    Success,
    /// 警告提示（黄色）
    Warning,
    /// 错误提示（红色）
    Error,
}

impl ToastLevel {
    /// 获取级别对应的颜色
    pub fn color(&self) -> u32 {
        match self {
            ToastLevel::Info => 0x89b4fa,    // Catppuccin Blue
            ToastLevel::Success => 0xa6e3a1, // Catppuccin Green
            ToastLevel::Warning => 0xf9e2af, // Catppuccin Yellow
            ToastLevel::Error => 0xf38ba8,   // Catppuccin Red
        }
    }

    /// 获取级别对应的图标
    pub fn icon(&self) -> &'static str {
        match self {
            ToastLevel::Info => "ℹ️",
            ToastLevel::Success => "✅",
            ToastLevel::Warning => "⚠️",
            ToastLevel::Error => "❌",
        }
    }
}

/// Toast - 队列消息提示
///
/// Toast 是轻量级的消息提示组件，以队列形式显示在屏幕顶部或底部。
/// 多个 Toast 会按顺序排列，自动消失或手动关闭。
///
/// # 特性
/// - 队列显示：多个 Toast 按顺序排列
/// - 自动消失：可设置自动消失时间
/// - 分级显示：Info/Success/Warning/Error 不同样式
/// - 可关闭：支持手动关闭
///
/// # 示例
/// ```rust
/// // 显示成功提示
/// toast_queue.push(Toast::success("保存成功"));
///
/// // 显示错误提示，5秒后自动消失
/// toast_queue.push(Toast::error("网络连接失败").duration(5000));
///
/// // 显示信息提示，不自动消失
/// toast_queue.push(Toast::info("正在同步...").persistent());
/// ```
#[derive(Clone)]
pub struct Toast {
    /// Toast 唯一 ID
    id: String,
    /// 消息内容
    message: String,
    /// 消息级别
    level: ToastLevel,
    /// 持续时间（毫秒），None 表示不自动消失
    duration: Option<u64>,
    /// 创建时间
    created_at: Instant,
    /// 是否可关闭
    closable: bool,
}

impl Toast {
    /// 创建一个新的 Toast
    pub fn new(message: impl Into<String>, level: ToastLevel) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            message: message.into(),
            level,
            duration: Some(3000), // 默认 3 秒
            created_at: Instant::now(),
            closable: true,
        }
    }

    /// 创建信息提示
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Info)
    }

    /// 创建成功提示
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Success)
    }

    /// 创建警告提示
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Warning)
    }

    /// 创建错误提示
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, ToastLevel::Error)
    }

    /// 设置持续时间（毫秒）
    pub fn duration(mut self, ms: u64) -> Self {
        self.duration = Some(ms);
        self
    }

    /// 设置为持久显示（不自动消失）
    pub fn persistent(mut self) -> Self {
        self.duration = None;
        self
    }

    /// 设置是否可关闭
    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }

    /// 获取 Toast ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 获取消息内容
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 获取消息级别
    pub fn level(&self) -> ToastLevel {
        self.level
    }

    /// 检查是否已过期
    pub fn is_expired(&self) -> bool {
        if let Some(duration) = self.duration {
            self.created_at.elapsed() > Duration::from_millis(duration)
        } else {
            false
        }
    }

    /// 检查是否可关闭
    pub fn is_closable(&self) -> bool {
        self.closable
    }

    /// 渲染 Toast（使用主题）
    pub fn render(&self, theme: &Theme) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(theme.spacing.sm)
            .px(theme.spacing.md)
            .py(theme.spacing.sm)
            .bg(rgb(theme.colors.surface))
            .border_l_4()
            .border_color(rgb(self.level.color()))
            .rounded(theme.radius.md)
            .shadow_lg()
            .min_w(px(300.0))
            .max_w(px(500.0))
            .child(
                div()
                    .text_size(theme.typography.body.size)
                    .child(self.level.icon())
            )
            .child(
                div()
                    .flex_1()
                    .text_color(rgb(theme.colors.text))
                    .text_size(theme.typography.body.size)
                    .child(self.message.clone())
            )
            .when(self.closable, |this| {
                this.child(
                    div()
                        .px(theme.spacing.xs)
                        .cursor_pointer()
                        .text_color(rgb(theme.colors.text_secondary))
                        .hover(|style| style.text_color(rgb(theme.colors.text)))
                        .child("✕")
                )
            })
    }
}

/// ToastQueue - Toast 队列管理器
///
/// 管理 Toast 的显示队列，自动处理过期的 Toast
pub struct ToastQueue {
    toasts: VecDeque<Toast>,
    max_count: usize,
}

impl ToastQueue {
    /// 创建一个新的 Toast 队列
    pub fn new() -> Self {
        Self {
            toasts: VecDeque::new(),
            max_count: 5, // 默认最多显示 5 个
        }
    }

    /// 设置最大显示数量
    pub fn with_max_count(mut self, max_count: usize) -> Self {
        self.max_count = max_count;
        self
    }

    /// 添加一个 Toast 到队列
    pub fn push(&mut self, toast: Toast) {
        // 如果队列已满，移除最旧的
        if self.toasts.len() >= self.max_count {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    /// 移除指定 ID 的 Toast
    pub fn remove(&mut self, id: &str) {
        self.toasts.retain(|t| t.id() != id);
    }

    /// 清空所有 Toast
    pub fn clear(&mut self) {
        self.toasts.clear();
    }

    /// 更新队列，移除过期的 Toast
    pub fn update(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    /// 获取当前所有 Toast
    pub fn toasts(&self) -> &VecDeque<Toast> {
        &self.toasts
    }

    /// 获取 Toast 数量
    pub fn len(&self) -> usize {
        self.toasts.len()
    }

    /// 检查队列是否为空
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// 渲染 Toast 队列（使用主题）
    pub fn render(&self, theme: &Theme) -> impl IntoElement {
        div()
            .absolute()
            .top(theme.spacing.md)
            .right(theme.spacing.md)
            .flex()
            .flex_col()
            .gap(theme.spacing.sm)
            // z-index 在 GPUI 中通过层级关系自动处理
            .children(
                self.toasts.iter().map(|toast| toast.render(theme))
            )
    }
}

impl Default for ToastQueue {
    fn default() -> Self {
        Self::new()
    }
}
