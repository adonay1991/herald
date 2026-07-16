# Changelog

All notable changes to herald are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow SemVer.

## [0.1.0] — 2026-07-16

First public release.

### Core

- Canonical `Event` model (`source`, `kind`, `urgency`, `title`, `body`,
  `cwd`, `session_id`, `agent_label`, `raw`) with an additive-only stability
  contract (docs/CONTRACT.md).
- Pure routing: critical → always, normal → only when unfocused (or
  headless), low → log only; harnesses with their own notification UI own
  the pane and suppress the system banner.
- **Burst coalescing**: an identical (source, kind) delivered within
  `burst-window-ms` (default 2000, 0 disables) is suppressed — fan-outs and
  parallel sessions produce one banner, not N.
- **Quiet hours**: `quiet-hours = "23:00-08:00"` silences everything below
  critical inside the window (wraps midnight).
- **tmux awareness**: inside tmux the terminal-app focus answer lies, so
  focus is treated as unknown and herald errs on the side of notifying.
- Always-on JSONL logbook with **size-capped rotation** (5 MB → `.1`).

### Adapters

- Claude Code (`herald hook claude`, stdin JSON), Codex CLI
  (`herald hook codex`, argv JSON), Gemini CLI (experimental), and the
  universal contract `herald emit --json` / flags.

### Sinks

- `system`: macOS presenter app (UNUserNotificationCenter — the only API
  macOS 26 still honors) → terminal-notifier → osascript cascade; **Linux
  notify-send backend** with 1:1 urgency mapping. Per-urgency
  **configurable sounds**.
- `herdr` (`herdr notification show`) and `cmux` (`cmux notify`) muxer sinks.
- **`osc` sink (opt-in)**: OSC 9 / OSC 777 escape sequences written to
  /dev/tty — the terminal decides presentation and focus; event text is
  sanitized so it can never inject its own escape sequences.
- Declarative `exec` sinks for third-party terminals (docs/SINKS.md).

### Presenter (macOS)

- Herald.app source + `herald doctor --install-app` (builds with swiftc,
  installs alongside any existing presenter, never replaces one).
- **Click-to-activate**: clicking a banner activates the originating
  terminal (bundle id travels via `HERALD_ACTIVATE_BUNDLE` → notification
  userInfo → relaunch delegate).

### Tooling

- `herald doctor [--json]` — full pipeline diagnosis, machine-readable on
  demand.
- `herald log [-n N] [-f] [--raw] [--stats]` — inspect, tail or aggregate
  the events log.
- `herald test [--kind] [--sink]`, global `--dry-run` (prints the exact
  delivery plan as JSON).
- Focus detection: macOS (`lsappinfo`), Linux X11 (`WINDOWID` + `xdotool`);
  Wayland reports unknown (herald notifies rather than risk silence).
- CI: fmt + clippy (`-D warnings`) + tests on macOS and Linux.
