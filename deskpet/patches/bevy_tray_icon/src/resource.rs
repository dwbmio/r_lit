//! Resources used by this plugin.

use bevy::asset::{Assets, Handle};
use bevy::prelude::{Deref, DerefMut, Image, Resource};
use tray_icon::{menu, Icon};

pub use menu_item::{MenuIconKind, MenuItem};

mod menu_item;

/// After this resource is generated, the system tray is also generated.
///
/// When you change a field in this resource, the system tray is also updated.

#[derive(Debug, Clone, PartialEq, Default, Resource)]
pub struct TrayIcon {
    /// The image handle of icon.
    pub icon: Option<Handle<Image>>,

    /// The text in tooltip displayed when mouse hovers over the system tray.
    pub tooltip: Option<String>,

    /// The menu displayed when right-clicking on the system tray.
    pub menu: Menu,

    /// Whether to show the menu when left-clicking on the system tray.
    /// This flag is only effective on macOS.
    pub show_menu_on_left_click: bool,
}

impl TrayIcon {
    pub(crate) fn as_icon(&self, images: &Assets<Image>) -> Option<Icon> {
        let id = self.icon.as_ref().map(|handle| handle.id())?;
        let image = images.get(id)?;
        Icon::from_rgba(image.data.clone()?, image.width(), image.height()).ok()
    }
}

/// The system tray's menu.
#[derive(Debug, Clone, PartialEq, Default, Deref, DerefMut)]
pub struct Menu(Vec<MenuItem>);

impl Menu {
    /// Creates the new [`Menu`].
    #[inline]
    pub const fn new(items: Vec<MenuItem>) -> Self {
        Self(items)
    }

    /// Enables or disables the specified menu.
    pub fn set_enable(&mut self, menu_id: &str, enable: bool) -> crate::error::Result {
        for menu in self.0.iter_mut() {
            match menu {
                MenuItem::Common { id, enabled, .. }
                | MenuItem::Check { id, enabled, .. }
                | MenuItem::Icon { id, enabled, .. } => {
                    if id.0 == menu_id {
                        *enabled = enable;
                        return Ok(());
                    }
                }
                MenuItem::SubMenu {
                    id, enabled, menu, ..
                } => {
                    if id.0 == menu_id {
                        *enabled = enable;
                        return Ok(());
                    } else if menu.set_enable(menu_id, enable).is_ok() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        Err(crate::error::Error::NotFoundMenu(menu_id.to_string()))
    }

    /// Sets the checked state of a checkbox menu item.
    pub fn set_checked(&mut self, menu_id: &str, checked: bool) -> crate::error::Result {
        for menu in self.0.iter_mut() {
            match menu {
                MenuItem::Check { id, checked: c, .. } if id.0 == menu_id => {
                    *c = checked;
                    return Ok(());
                }
                MenuItem::SubMenu { menu, .. } => {
                    if menu.set_checked(menu_id, checked).is_ok() {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        Err(crate::error::Error::NotFoundMenu(menu_id.to_string()))
    }

    /// Returns the checked state of a checkbox menu item, if present.
    pub fn is_checked(&self, menu_id: &str) -> Option<bool> {
        for menu in self.0.iter() {
            match menu {
                MenuItem::Check { id, checked, .. } if id.0 == menu_id => return Some(*checked),
                MenuItem::SubMenu { menu, .. } => {
                    if let Some(v) = menu.is_checked(menu_id) {
                        return Some(v);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn as_context(&self) -> Option<Box<dyn menu::ContextMenu>> {
        if self.0.is_empty() {
            None
        } else {
            let menu = menu::Menu::new();
            for item in self.0.iter().map(|item| item.as_dyn()) {
                menu.append(&*item).unwrap();
            }
            Some(Box::new(menu))
        }
    }
}

#[repr(transparent)]
#[derive(Deref)]
pub(crate) struct RealTrayIcon(pub(crate) tray_icon::TrayIcon);

/// macOS 菜单内「运行中」绿点（RMB 风格）。
pub fn green_status_dot_icon() -> menu::Icon {
    use std::sync::OnceLock;
    static ICON: OnceLock<menu::Icon> = OnceLock::new();
    ICON.get_or_init(|| {
        const S: u32 = 18;
        let mut rgba = vec![0u8; (S * S * 4) as usize];
        let cx = (S as f32 - 1.0) / 2.0;
        let r = 7.0_f32;
        for y in 0..S {
            for x in 0..S {
                let dx = x as f32 - cx;
                let dy = y as f32 - cx;
                let d = (dx * dx + dy * dy).sqrt();
                if d > r {
                    continue;
                }
                let t = (d / r).clamp(0.0, 1.0);
                let hl = (1.0 - t * 0.55)
                    * (1.0 - ((dx + 2.5).powi(2) + (dy + 2.5).powi(2)).sqrt() / r * 0.35)
                        .clamp(0.0, 1.0);
                let g = (48.0 + 190.0 * hl) as u8;
                let rd = (24.0 + 50.0 * hl) as u8;
                let i = ((y * S + x) * 4) as usize;
                rgba[i] = rd;
                rgba[i + 1] = g;
                rgba[i + 2] = 36;
                rgba[i + 3] = if d <= r - 0.6 {
                    255
                } else {
                    ((r - d) * 400.0).clamp(0.0, 255.0) as u8
                };
            }
        }
        menu::Icon::from_rgba(rgba, S, S).expect("green status dot")
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use crate::resource::{Menu, MenuItem};

    #[test]
    fn enable_common() {
        let mut menu = Menu::new(vec![MenuItem::common("test", "test", false, None)]);
        menu.set_enable("test", true).unwrap();
        assert!(check_enabled("test", &menu));
    }

    #[test]
    fn enable_submenu() {
        let mut menu = Menu::new(vec![
            MenuItem::common("test", "test", false, None),
            MenuItem::submenu("sub", "sub", false, Menu::default()),
        ]);
        menu.set_enable("sub", true).unwrap();
        assert!(check_enabled("sub", &menu));
    }

    #[test]
    fn enable_item_in_submenu() {
        let mut menu = Menu::new(vec![
            MenuItem::common("test", "test", false, None),
            MenuItem::submenu(
                "sub",
                "sub",
                false,
                Menu::new(vec![MenuItem::common("sub_item", "sub_item", false, None)]),
            ),
        ]);
        menu.set_enable("sub_item", true).unwrap();
        assert!(check_enabled("sub_item", &menu));
    }

    fn check_enabled(target_id: &str, menu: &Menu) -> bool {
        for item in menu.0.iter() {
            match item {
                MenuItem::SubMenu {
                    id, menu, enabled, ..
                } => {
                    if id == target_id {
                        return *enabled;
                    } else if check_enabled(target_id, menu) {
                        return true;
                    }
                }
                MenuItem::Common { id, enabled, .. } => {
                    if id == target_id {
                        return *enabled;
                    }
                }
                _ => {}
            }
        }
        false
    }
}
