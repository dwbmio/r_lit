// GUI 模块 - 用户界面组件系统
//
// 组件颗粒度层级：
// - Page: 全屏页面，独立路由
// - View: Page 上的全屏区域
// - PopView: 浮层弹窗，互斥显示
// - Toast: 队列消息提示
//
// 主题系统：
// - Theme: 统一的主题定义，包含颜色、字体、间距等
// - 所有组件样式都从主题中获取，确保视觉一致性

pub mod page;
pub mod view;
pub mod popview;
pub mod toast;
pub mod router;
pub mod theme;
pub mod pages;
pub mod popviews;
pub mod components;

pub use page::Page;
pub use view::View;
pub use popview::PopView;
pub use toast::{Toast, ToastQueue, ToastLevel};
pub use router::Router;
pub use theme::{Theme, ThemeManager, ColorScheme, Typography, Spacing};
