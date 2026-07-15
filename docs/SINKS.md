# herald — Terminal / Sink Integration Contract

herald decides *where* a notification goes by detecting which terminal harness
owns the pane, then hands the event to that harness's sink. **Any terminal can
integrate by exporting one environment variable and providing one command.**

## The exec sink contract

Add to `~/.config/herald/config.toml`:

```toml
[[sinks.exec]]
name = "myterm"                    # shows up in logs as exec:myterm
when_env = "MYTERM_SOCKET"         # sink is active when this env var is set
command = ["myterm-notify", "--stdin"]
min_urgency = "normal"             # optional: low | normal | critical
exclusive = true                   # optional: acts as a harness — suppresses the system banner
```

When active, `command` runs with the canonical Event JSON (docs/CONTRACT.md)
on **stdin**, with a 2-second budget. Exit 0 means delivered. herald inherits
the pane's environment, so your command can read your own env vars (socket
paths, pane ids) directly.

`exclusive = true` means "this terminal owns the user's attention": the system
banner is suppressed, exactly like the built-in muxer sinks. Non-exclusive
sinks deliver *in addition to* whatever else routing decides.

## Built-in sinks

The built-ins implement the same contract with optimized transports:

| Sink | Detection | Transport |
|---|---|---|
| `herdr` | `HERDR_ENV=1` | `herdr notification show <title> --body … --sound none\|done\|request` |
| `cmux` | `CMUX_SURFACE_ID` | `cmux notify --title … --body …` |
| `system` | fallback | cascade: presenter app → terminal-notifier → osascript |
| `exec:*` | `when_env` | your command, Event JSON on stdin |

**orca** (`ORCA_AGENT_HOOK_PORT`) is detected but is deliberately *not* a sink:
orca consumes agent state through its own hook channel. Under orca, herald
shows system banners only for `always`-policy events (attention/error) and
suppresses turn-complete noise.

## Routing (zero-config defaults)

1. Everything is logged to `~/.local/state/herald/events.jsonl`.
2. A harness sink (herdr, cmux, exclusive exec) owns the pane → system banner suppressed.
3. No harness: `critical` → notify always; `normal` → notify unless the
   terminal is frontmost; `low` → log only.
4. Headless (no terminal app, no tty — launchd, cron, CI) → always deliver;
   banners persist in Notification Center until seen.

Per-kind overrides, no rule engine:

```toml
[routing]
turn-complete = "never"     # always | unfocused | never
```

## The system sink on macOS 26+

The legacy notification API (osascript, terminal-notifier) is dead on macOS 26:
it exits 0 and shows nothing. The system sink's primary backend is a minimal
presenter app using `UNUserNotificationCenter` (source in `app/`). Point
herald at any working presenter bundle:

```toml
[sinks.macos_native]
app_path = "/Users/me/Applications/Herald.app"
```

The presenter contract is CLI-level: `<binary> <title> <message> [sound]` plus
`<binary> status` printing `authorizationStatus: …`. herald resolves the
binary as the first file in `Contents/MacOS`, so any conforming bundle works.
Check health with `herald doctor`.
