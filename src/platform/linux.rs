//! Focus detection on Linux. X11 has a reliable, dependency-light answer:
//! most terminals export WINDOWID, and `xdotool getactivewindow` prints the
//! focused window id — equal means focused. Wayland has no portable protocol
//! for this (compositor-specific: hyprctl, KWin scripting, GNOME extensions),
//! so herald returns "unknown" there and errs on the side of notifying.

use std::process::Command;

pub fn frontmost() -> Option<bool> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return None;
    }
    let window_id = std::env::var("WINDOWID").ok()?;
    let out = Command::new("xdotool")
        .arg("getactivewindow")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let active = String::from_utf8_lossy(&out.stdout);
    Some(active.trim() == window_id.trim())
}
