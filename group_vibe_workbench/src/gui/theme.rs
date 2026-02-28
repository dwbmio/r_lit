use gpui::{px, rgb, Pixels};

/// Theme - 应用主题系统
///
/// 统一管理应用的所有视觉样式，包括颜色、字体、间距、圆角等。
/// 所有组件的样式都应该从主题中获取，确保视觉一致性。
///
/// # 设计原则
/// - 单一数据源：所有样式定义都来自主题
/// - 语义化命名：使用语义化的颜色名称（如 primary, success）而非具体颜色值
/// - 可切换：支持多套主题（如 Light/Dark/Custom）
/// - 分层设计：颜色、字体、间距等分类管理
///
/// # 示例
/// ```rust
/// let theme = Theme::catppuccin_mocha();
///
/// // 使用主题颜色
/// div().bg(rgb(theme.colors.background));
///
/// // 使用主题字体
/// div().text_size(theme.typography.body.size);
///
/// // 使用主题间距
/// div().p(theme.spacing.md);
/// ```
#[derive(Clone, Debug)]
pub struct Theme {
    /// 主题名称
    pub name: String,
    /// 颜色系统
    pub colors: ColorScheme,
    /// 字体系统
    pub typography: Typography,
    /// 间距系统
    pub spacing: Spacing,
    /// 圆角系统
    pub radius: Radius,
    /// 阴影系统
    pub shadow: Shadow,
    /// 动画系统
    pub animation: Animation,
}

impl Theme {
    /// Catppuccin Mocha 主题（默认深色主题）
    pub fn catppuccin_mocha() -> Self {
        Self {
            name: "Catppuccin Mocha".to_string(),
            colors: ColorScheme::catppuccin_mocha(),
            typography: Typography::default(),
            spacing: Spacing::default(),
            radius: Radius::default(),
            shadow: Shadow::default(),
            animation: Animation::default(),
        }
    }

    /// Catppuccin Latte 主题（浅色主题）
    pub fn catppuccin_latte() -> Self {
        Self {
            name: "Catppuccin Latte".to_string(),
            colors: ColorScheme::catppuccin_latte(),
            typography: Typography::default(),
            spacing: Spacing::default(),
            radius: Radius::default(),
            shadow: Shadow::default(),
            animation: Animation::default(),
        }
    }

    /// 自定义主题
    pub fn custom(name: impl Into<String>, colors: ColorScheme) -> Self {
        Self {
            name: name.into(),
            colors,
            typography: Typography::default(),
            spacing: Spacing::default(),
            radius: Radius::default(),
            shadow: Shadow::default(),
            animation: Animation::default(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::catppuccin_mocha()
    }
}

/// ColorScheme - 颜色方案
///
/// 定义应用的所有颜色，使用语义化命名
#[derive(Clone, Debug)]
pub struct ColorScheme {
    // === 基础颜色 ===
    /// 主背景色
    pub background: u32,
    /// 次级背景色（卡片、面板等）
    pub surface: u32,
    /// 三级背景色（悬停、选中等）
    pub surface_variant: u32,

    // === 文本颜色 ===
    /// 主文本颜色
    pub text: u32,
    /// 次级文本颜色（说明文字等）
    pub text_secondary: u32,
    /// 禁用文本颜色
    pub text_disabled: u32,

    // === 边框颜色 ===
    /// 主边框颜色
    pub border: u32,
    /// 分割线颜色
    pub divider: u32,

    // === 语义颜色 ===
    /// 主色调（品牌色）
    pub primary: u32,
    /// 主色调悬停态
    pub primary_hover: u32,
    /// 成功色
    pub success: u32,
    /// 警告色
    pub warning: u32,
    /// 错误色
    pub error: u32,
    /// 信息色
    pub info: u32,

    // === 遮罩颜色 ===
    /// 遮罩层背景色（带透明度）
    pub mask: u32,
    /// 遮罩层透明度 (0.0 - 1.0)
    pub mask_opacity: f32,
}

impl ColorScheme {
    /// Catppuccin Mocha 配色方案
    pub fn catppuccin_mocha() -> Self {
        Self {
            background: 0x1e1e2e,
            surface: 0x313244,
            surface_variant: 0x45475a,

            text: 0xcdd6f4,
            text_secondary: 0xbac2de,
            text_disabled: 0x6c7086,

            border: 0x45475a,
            divider: 0x313244,

            primary: 0x89b4fa,
            primary_hover: 0x74c7ec,
            success: 0xa6e3a1,
            warning: 0xf9e2af,
            error: 0xf38ba8,
            info: 0x89dceb,

            mask: 0x000000,
            mask_opacity: 0.5,
        }
    }

    /// Catppuccin Latte 配色方案（浅色）
    pub fn catppuccin_latte() -> Self {
        Self {
            background: 0xeff1f5,
            surface: 0xe6e9ef,
            surface_variant: 0xdce0e8,

            text: 0x4c4f69,
            text_secondary: 0x5c5f77,
            text_disabled: 0x9ca0b0,

            border: 0xdce0e8,
            divider: 0xe6e9ef,

            primary: 0x1e66f5,
            primary_hover: 0x04a5e5,
            success: 0x40a02b,
            warning: 0xdf8e1d,
            error: 0xd20f39,
            info: 0x209fb5,

            mask: 0x000000,
            mask_opacity: 0.3,
        }
    }
}

/// Typography - 字体系统
///
/// 定义应用的字体大小、行高、字重等
#[derive(Clone, Debug)]
pub struct Typography {
    /// 字体族
    pub font_family: String,
    /// 等宽字体族（代码等）
    pub font_family_mono: String,

    /// 标题样式
    pub heading: TextStyle,
    /// 副标题样式
    pub subheading: TextStyle,
    /// 正文样式
    pub body: TextStyle,
    /// 小字样式
    pub caption: TextStyle,
    /// 按钮文字样式
    pub button: TextStyle,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            font_family: "system-ui, -apple-system, sans-serif".to_string(),
            font_family_mono: "ui-monospace, monospace".to_string(),

            heading: TextStyle {
                size: px(24.0),
                line_height: 1.3,
                weight: 600,
            },
            subheading: TextStyle {
                size: px(18.0),
                line_height: 1.4,
                weight: 500,
            },
            body: TextStyle {
                size: px(14.0),
                line_height: 1.5,
                weight: 400,
            },
            caption: TextStyle {
                size: px(12.0),
                line_height: 1.4,
                weight: 400,
            },
            button: TextStyle {
                size: px(14.0),
                line_height: 1.0,
                weight: 500,
            },
        }
    }
}

/// TextStyle - 文本样式
#[derive(Clone, Debug)]
pub struct TextStyle {
    /// 字体大小
    pub size: Pixels,
    /// 行高（相对于字体大小的倍数）
    pub line_height: f32,
    /// 字重
    pub weight: u16,
}

/// Spacing - 间距系统
///
/// 定义应用的标准间距值，使用 8px 基准
#[derive(Clone, Debug)]
pub struct Spacing {
    /// 超小间距 (4px)
    pub xs: Pixels,
    /// 小间距 (8px)
    pub sm: Pixels,
    /// 中等间距 (16px)
    pub md: Pixels,
    /// 大间距 (24px)
    pub lg: Pixels,
    /// 超大间距 (32px)
    pub xl: Pixels,
    /// 巨大间距 (48px)
    pub xxl: Pixels,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            xs: px(4.0),
            sm: px(8.0),
            md: px(16.0),
            lg: px(24.0),
            xl: px(32.0),
            xxl: px(48.0),
        }
    }
}

/// Radius - 圆角系统
///
/// 定义应用的标准圆角值
#[derive(Clone, Debug)]
pub struct Radius {
    /// 无圆角
    pub none: Pixels,
    /// 小圆角 (4px)
    pub sm: Pixels,
    /// 中等圆角 (8px)
    pub md: Pixels,
    /// 大圆角 (12px)
    pub lg: Pixels,
    /// 完全圆角 (9999px)
    pub full: Pixels,
}

impl Default for Radius {
    fn default() -> Self {
        Self {
            none: px(0.0),
            sm: px(4.0),
            md: px(8.0),
            lg: px(12.0),
            full: px(9999.0),
        }
    }
}

/// Shadow - 阴影系统
///
/// 定义应用的标准阴影效果
#[derive(Clone, Debug)]
pub struct Shadow {
    /// 无阴影
    pub none: String,
    /// 小阴影
    pub sm: String,
    /// 中等阴影
    pub md: String,
    /// 大阴影
    pub lg: String,
}

impl Default for Shadow {
    fn default() -> Self {
        Self {
            none: "none".to_string(),
            sm: "0 1px 2px 0 rgba(0, 0, 0, 0.05)".to_string(),
            md: "0 4px 6px -1px rgba(0, 0, 0, 0.1)".to_string(),
            lg: "0 10px 15px -3px rgba(0, 0, 0, 0.1)".to_string(),
        }
    }
}

/// Animation - 动画系统
///
/// 定义应用的标准动画时长和缓动函数
#[derive(Clone, Debug)]
pub struct Animation {
    /// 快速动画 (150ms)
    pub fast: u64,
    /// 正常动画 (300ms)
    pub normal: u64,
    /// 慢速动画 (500ms)
    pub slow: u64,
    /// 缓动函数
    pub easing: String,
}

impl Default for Animation {
    fn default() -> Self {
        Self {
            fast: 150,
            normal: 300,
            slow: 500,
            easing: "cubic-bezier(0.4, 0, 0.2, 1)".to_string(),
        }
    }
}

/// ThemeManager - 主题管理器
///
/// 管理应用的主题切换和持久化
pub struct ThemeManager {
    current_theme: Theme,
    available_themes: Vec<Theme>,
}

impl ThemeManager {
    /// 创建主题管理器
    pub fn new() -> Self {
        Self {
            current_theme: Theme::default(),
            available_themes: vec![
                Theme::catppuccin_mocha(),
                Theme::catppuccin_latte(),
            ],
        }
    }

    /// 获取当前主题
    pub fn current(&self) -> &Theme {
        &self.current_theme
    }

    /// 切换主题
    pub fn switch(&mut self, theme_name: &str) {
        if let Some(theme) = self.available_themes.iter().find(|t| t.name == theme_name) {
            self.current_theme = theme.clone();
            log::info!("Switched to theme: {}", theme_name);
        } else {
            log::warn!("Theme not found: {}", theme_name);
        }
    }

    /// 添加自定义主题
    pub fn add_theme(&mut self, theme: Theme) {
        self.available_themes.push(theme);
    }

    /// 获取所有可用主题
    pub fn available_themes(&self) -> &[Theme] {
        &self.available_themes
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}
