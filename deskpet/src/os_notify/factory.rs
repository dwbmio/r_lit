//! Build the right `OsNotifier` for the (target_os, feature) combination.
//!
//! Coverage matrix (compile-time exhaustive):
//!
//! | target_os | feature os-notify-off | impl                            |
//! |-----------|-----------------------|---------------------------------|
//! | macos     | off                   | `macos::MacOsNotifier`          |
//! | windows   | off                   | `windows::WindowsNotifier`      |
//! | macos     | on                    | `other::NoopNotifier`           |
//! | windows   | on                    | `other::NoopNotifier`           |
//! | linux / * | on or off             | `other::NoopNotifier`           |

use std::sync::Arc;

use super::OsNotifier;

#[cfg(all(target_os = "macos", not(feature = "os-notify-off")))]
use super::macos::MacOsNotifier;
#[cfg(all(target_os = "windows", not(feature = "os-notify-off")))]
use super::windows::WindowsNotifier;
#[cfg(any(
    not(any(target_os = "macos", target_os = "windows")),
    feature = "os-notify-off"
))]
use super::other::NoopNotifier;

pub fn make_notifier() -> Arc<dyn OsNotifier> {
    make_notifier_impl()
}

#[cfg(all(target_os = "macos", not(feature = "os-notify-off")))]
fn make_notifier_impl() -> Arc<dyn OsNotifier> {
    Arc::new(MacOsNotifier::new())
}

#[cfg(all(target_os = "windows", not(feature = "os-notify-off")))]
fn make_notifier_impl() -> Arc<dyn OsNotifier> {
    Arc::new(WindowsNotifier::new())
}

#[cfg(any(
    not(any(target_os = "macos", target_os = "windows")),
    feature = "os-notify-off"
))]
fn make_notifier_impl() -> Arc<dyn OsNotifier> {
    Arc::new(NoopNotifier::new())
}