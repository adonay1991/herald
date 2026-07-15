//! Platform-specific effects, kept out of the pure detection/routing layers.
//! macOS is complete; Linux is a defined interface with a stub implementation
//! (see docs/CONTRACT.md for the intended notify-send mapping).

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod other;

use crate::context::Context;

/// Is the terminal app that owns this process currently frontmost?
/// `None` means "could not determine" — routing treats that as not-frontmost,
/// because notifying twice is better than losing an actionable alert.
pub fn terminal_is_frontmost(ctx: &Context) -> Option<bool> {
    let bundle = ctx.terminal_bundle_id.as_deref()?;
    imp_frontmost(bundle)
}

#[cfg(target_os = "macos")]
use macos::frontmost as imp_frontmost;
#[cfg(not(target_os = "macos"))]
use other::frontmost as imp_frontmost;
