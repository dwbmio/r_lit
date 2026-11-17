use tray_icon::menu;
use tray_icon::menu::accelerator::Accelerator;
use tray_icon::menu::{CheckMenuItem, IconMenuItem, IsMenuItem, MenuId, PredefinedMenuItem, Submenu};

use crate::resource::Menu;

/// Built-in menu-leading icons resolved at render time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MenuIconKind {
    GreenStatusDot,
}

/// System Tray Menu Item.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MenuItem {
    /// The menu common item.
    Common {
        /// Menu id.
        id: MenuId,
        /// The text displayed in the menu.
        text: String,
        /// whether you can click on this item.
        enabled: bool,
        /// A keyboard shortcut that consists of an optional combination of modifier keys (provided by Modifiers) and one key (Code).
        accelerator: Option<Accelerator>,
    },

    /// Submenu.
    SubMenu {
        /// Menu id
        id: MenuId,
        /// The text displayed in the menu.
        text: String,
        /// whether you can click on this item.
        enabled: bool,
        /// A keyboard shortcut that consists of an optional combination of modifier keys (provided by Modifiers) and one key (Code).
        menu: Menu,
    },

    /// The menu separator.
    Separator,

    /// Checkbox menu item (toggle).
    Check {
        id: MenuId,
        text: String,
        enabled: bool,
        checked: bool,
    },

    /// Menu item with a leading icon (e.g. status green dot).
    Icon {
        id: MenuId,
        text: String,
        enabled: bool,
        icon: MenuIconKind,
    },
}

impl MenuItem {
    /// Creates the common item.
    ///
    /// ## Params
    ///
    /// * `id` - Menu-id.
    /// * `text` - The text displayed in the menu.
    /// * `enabled` - whether you can click on this item.
    /// * `accelerator` - A keyboard shortcut that consists of an optional combination of modifier keys (provided by Modifiers) and one key (Code).
    #[inline]
    pub fn common(
        id: impl Into<MenuId>,
        text: impl Into<String>,
        enabled: bool,
        accelerator: Option<Accelerator>,
    ) -> Self {
        Self::Common {
            id: id.into(),
            text: text.into(),
            enabled,
            accelerator,
        }
    }

    /// Create the separator.
    #[inline]
    pub const fn separator() -> Self {
        Self::Separator
    }

    /// Creates a checkbox item.
    #[inline]
    pub fn check(
        id: impl Into<MenuId>,
        text: impl Into<String>,
        enabled: bool,
        checked: bool,
    ) -> Self {
        Self::Check {
            id: id.into(),
            text: text.into(),
            enabled,
            checked,
        }
    }

    /// Creates an item with a leading icon.
    #[inline]
    pub fn icon(
        id: impl Into<MenuId>,
        text: impl Into<String>,
        enabled: bool,
        icon: MenuIconKind,
    ) -> Self {
        Self::Icon {
            id: id.into(),
            text: text.into(),
            enabled,
            icon,
        }
    }

    /// Creates the submenu.
    ///
    /// ## Params
    ///
    /// * `id` - Menu-id.
    /// * `text` - The text displayed in the menu.
    /// * `enabled` - whether you can click on this item.
    /// * `menu` - Submenu
    #[inline]
    pub fn submenu(
        id: impl Into<MenuId>,
        text: impl Into<String>,
        enabled: bool,
        menu: Menu,
    ) -> Self {
        Self::SubMenu {
            id: id.into(),
            text: text.into(),
            enabled,
            menu,
        }
    }

    pub(super) fn as_dyn(&self) -> Box<dyn IsMenuItem> {
        match self {
            MenuItem::Separator => Box::new(PredefinedMenuItem::separator()),
            MenuItem::SubMenu {
                id,
                text,
                enabled,
                menu,
            } => {
                let sub_menu = Submenu::with_id(&id.0, text, *enabled);
                for item in menu.0.iter().map(|item| item.as_dyn()) {
                    sub_menu.append(&*item).unwrap();
                }
                Box::new(sub_menu)
            }
            MenuItem::Common {
                id,
                text,
                enabled,
                accelerator,
            } => {
                let item = menu::MenuItem::with_id(&id.0, text, *enabled, *accelerator);
                Box::new(item)
            }
            MenuItem::Check {
                id,
                text,
                enabled,
                checked,
            } => Box::new(CheckMenuItem::with_id(
                &id.0,
                text,
                *enabled,
                *checked,
                None,
            )),
            MenuItem::Icon {
                id,
                text,
                enabled,
                icon,
            } => {
                let resolved = match icon {
                    MenuIconKind::GreenStatusDot => super::green_status_dot_icon(),
                };
                Box::new(IconMenuItem::with_id(
                    &id.0,
                    text,
                    *enabled,
                    Some(resolved),
                    None,
                ))
            }
        }
    }
}
