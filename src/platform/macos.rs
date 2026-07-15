//! Focus detection on macOS, byte-for-byte the notify-native.sh approach:
//! `lsappinfo info -only bundleid "$(lsappinfo front)"` compared against the
//! __CFBundleIdentifier inherited from the terminal app.

use std::process::Command;

pub fn frontmost(terminal_bundle: &str) -> Option<bool> {
    let front = run(&["lsappinfo", "front"])?;
    let asn = front.trim();
    if asn.is_empty() {
        return None;
    }
    let info = run(&["lsappinfo", "info", "-only", "bundleid", asn])?;
    let front_bundle = parse_bundleid(&info)?;
    Some(front_bundle == terminal_bundle)
}

fn run(argv: &[&str]) -> Option<String> {
    let out = Command::new(argv[0]).args(&argv[1..]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// lsappinfo prints: "CFBundleIdentifier"="com.googlecode.iterm2"
fn parse_bundleid(raw: &str) -> Option<String> {
    let value = raw.trim().rsplit('=').next()?;
    let cleaned = value.trim().trim_matches('"');
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_bundleid;

    #[test]
    fn parses_lsappinfo_output() {
        assert_eq!(
            parse_bundleid("\"CFBundleIdentifier\"=\"com.googlecode.iterm2\"\n").as_deref(),
            Some("com.googlecode.iterm2")
        );
    }

    #[test]
    fn empty_output_is_none() {
        assert_eq!(parse_bundleid(""), None);
    }
}
