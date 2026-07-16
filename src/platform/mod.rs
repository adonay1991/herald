//! Platform-specific effects, kept out of the pure detection/routing layers.
//! Both platform modules are plain Command+string code, so they compile on
//! every OS and the branch is chosen with runtime-const `cfg!` — that way a
//! macOS build still type-checks the Linux path and vice versa.

pub mod linux;
pub mod macos;

use crate::context::Context;

/// Is the terminal that owns this process currently focused?
/// `None` means "could not determine" — routing treats that as not-frontmost,
/// because notifying twice is better than losing an actionable alert.
pub fn terminal_is_frontmost(ctx: &Context) -> Option<bool> {
    if cfg!(target_os = "macos") {
        let bundle = ctx.terminal_bundle_id.as_deref()?;
        macos::frontmost(bundle)
    } else if cfg!(target_os = "linux") {
        linux::frontmost()
    } else {
        None
    }
}
