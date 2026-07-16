//! Execution-context detection: which terminal harness owns this pane,
//! whether we are headless, and which app bundle the terminal belongs to.
//! Detection is a pure function of the environment snapshot so it is
//! trivially testable; effects (tty probe) happen only in `current()`.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    /// herdr multiplexer (HERDR_ENV=1). Has its own notification UI.
    Herdr,
    /// cmux (CMUX_SURFACE_ID). Has its own notification UI.
    Cmux,
    /// orca (ORCA_AGENT_HOOK_PORT). Paints pane state via its own hooks;
    /// herald must not duplicate turn-complete noise, only actionable banners.
    Orca,
    /// A plain terminal or no terminal at all.
    Plain,
}

impl Harness {
    pub fn name(self) -> &'static str {
        match self {
            Harness::Herdr => "herdr",
            Harness::Cmux => "cmux",
            Harness::Orca => "orca",
            Harness::Plain => "plain",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Context {
    pub harness: Harness,
    /// __CFBundleIdentifier inherited from the terminal app on macOS.
    pub terminal_bundle_id: Option<String>,
    /// No terminal app and no controlling tty: launchd jobs, crons, CI.
    pub headless: bool,
    /// Inside tmux the terminal-app focus check lies (the app may be
    /// frontmost while the user sits in another tmux window), so focus is
    /// treated as unknown and herald errs on the side of notifying.
    pub tmux: bool,
}

/// Most specific harness first: a pane inside herdr may still carry
/// terminal-level vars, so muxer vars win over everything else.
pub fn detect(env: &HashMap<String, String>, has_tty: bool) -> Context {
    let harness = if env.get("HERDR_ENV").is_some_and(|v| v == "1") {
        Harness::Herdr
    } else if env.get("CMUX_SURFACE_ID").is_some_and(|v| !v.is_empty()) {
        Harness::Cmux
    } else if env
        .get("ORCA_AGENT_HOOK_PORT")
        .is_some_and(|v| !v.is_empty())
    {
        Harness::Orca
    } else {
        Harness::Plain
    };

    let terminal_bundle_id = env
        .get("__CFBundleIdentifier")
        .filter(|v| !v.is_empty())
        .cloned();
    let headless = terminal_bundle_id.is_none() && !has_tty;
    let tmux = env.get("TMUX").is_some_and(|v| !v.is_empty());

    Context {
        harness,
        terminal_bundle_id,
        headless,
        tmux,
    }
}

/// Snapshot the real environment. The tty probe opens /dev/tty: it fails
/// exactly when there is no controlling terminal (launchd, cron).
pub fn current() -> Context {
    let env: HashMap<String, String> = std::env::vars().collect();
    let has_tty = std::fs::File::open("/dev/tty").is_ok();
    detect(&env, has_tty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn herdr_wins_over_terminal_vars() {
        let ctx = detect(
            &env(&[
                ("HERDR_ENV", "1"),
                ("__CFBundleIdentifier", "com.apple.Terminal"),
            ]),
            true,
        );
        assert_eq!(ctx.harness, Harness::Herdr);
        assert!(!ctx.headless);
    }

    #[test]
    fn detection_order_herdr_cmux_orca_plain() {
        let all = env(&[
            ("HERDR_ENV", "1"),
            ("CMUX_SURFACE_ID", "s1"),
            ("ORCA_AGENT_HOOK_PORT", "4242"),
        ]);
        assert_eq!(detect(&all, true).harness, Harness::Herdr);
        let cmux = env(&[("CMUX_SURFACE_ID", "s1"), ("ORCA_AGENT_HOOK_PORT", "4242")]);
        assert_eq!(detect(&cmux, true).harness, Harness::Cmux);
        let orca = env(&[("ORCA_AGENT_HOOK_PORT", "4242")]);
        assert_eq!(detect(&orca, true).harness, Harness::Orca);
        assert_eq!(detect(&env(&[]), true).harness, Harness::Plain);
    }

    #[test]
    fn headless_needs_no_bundle_and_no_tty() {
        assert!(detect(&env(&[]), false).headless);
        assert!(!detect(&env(&[]), true).headless);
        assert!(
            !detect(
                &env(&[("__CFBundleIdentifier", "com.googlecode.iterm2")]),
                false
            )
            .headless
        );
    }

    #[test]
    fn herdr_env_must_be_exactly_one() {
        assert_eq!(
            detect(&env(&[("HERDR_ENV", "0")]), true).harness,
            Harness::Plain
        );
    }

    #[test]
    fn tmux_detected_from_env() {
        assert!(detect(&env(&[("TMUX", "/tmp/tmux-501/default,123,0")]), true).tmux);
        assert!(!detect(&env(&[]), true).tmux);
        assert!(!detect(&env(&[("TMUX", "")]), true).tmux);
    }
}
