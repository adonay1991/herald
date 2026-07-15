//! Non-macOS stub. Linux focus detection is compositor-specific
//! (hyprctl / xdotool / GNOME extensions); until implemented, herald cannot
//! determine focus and errs on the side of notifying.

pub fn frontmost(_terminal_bundle: &str) -> Option<bool> {
    None
}
