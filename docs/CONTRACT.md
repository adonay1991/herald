# herald — Agent Integration Contract

herald is an agent- and terminal-agnostic notification dispatcher. **Any agent
that can run a command can notify.** There is no plugin API and no mapping DSL:
you produce one JSON object — the canonical Event — and pipe it to
`herald emit --json`. A five-line wrapper beats a rules engine.

## The canonical Event

```json
{
  "source": "my-agent",
  "kind": "attention",
  "urgency": "critical",
  "title": "my-agent — myproject",
  "body": "Waiting for your approval to run migrations",
  "cwd": "/home/me/myproject",
  "session_id": "abc123",
  "agent_label": "planner",
  "raw": { "anything": "you want to pass through to sinks" }
}
```

| Field | Required | Meaning |
|---|---|---|
| `source` | yes | Who emits: `"claude"`, `"codex"`, `"cron:backup"`, your agent's name. |
| `kind` | yes | One of `attention`, `turn-complete`, `session-start`, `session-end`, `error`, `info`. |
| `body` | yes | Human-readable message (herald truncates long bodies for display). |
| `urgency` | no | `low` / `normal` / `critical`. Defaults from `kind` (see below). |
| `title` | no | Defaults to `"{source} — {basename(cwd)}"`. |
| `cwd` | no | Working directory; feeds the default title. |
| `session_id` | no | Opaque session identifier, logged for correlation. |
| `agent_label` | no | Sub-agent type, model name, etc. |
| `raw` | no | Original payload, passed through untouched to `exec` sinks. |

### Kind semantics and default urgency

| kind | means | default urgency | default behavior |
|---|---|---|---|
| `attention` | agent is blocked on a human | critical | always notify |
| `error` | something failed | critical | always notify |
| `turn-complete` | a turn/response finished | normal | notify only when the terminal is unfocused (or headless) |
| `info` | FYI | low | log only |
| `session-start` / `session-end` | lifecycle | low | log only |

Urgency drives delivery; kind carries semantics. Overriding `urgency` changes
delivery without lying about what happened.

## Emitting

```sh
# Flags (for shell one-liners and crons):
herald emit --source cron:backup --kind error --body "backup failed"

# Canonical JSON (for wrappers):
echo '{"source":"my-agent","kind":"attention","body":"need input"}' | herald emit --json
```

Exit codes: `emit` reports real errors. The `herald hook *` entry points always
exit 0 — a notification dispatcher must never break an agent's turn.

## Built-in adapters

Built-in adapters exist only for stable, well-known agent protocols; they are
not an extension surface (use `emit --json` instead):

| Command | Transport | Protocol |
|---|---|---|
| `herald hook claude` | JSON on stdin | Claude Code hooks (`hook_event_name` envelope) |
| `herald hook codex`  | JSON as final argv | Codex CLI `notify` (`agent-turn-complete`) |
| `herald hook gemini` | JSON on stdin | Gemini CLI hooks (experimental) |

Wiring examples:

```jsonc
// Claude Code settings.json
"Notification": [{ "hooks": [{ "type": "command", "command": "herald hook claude" }] }],
"Stop":         [{ "hooks": [{ "type": "command", "command": "herald hook claude" }] }]
```

```toml
# Codex config.toml (root key, machine-local, absolute path — no ~ expansion)
notify = ["/abs/path/to/herald", "hook", "codex"]
```

## Stability guarantee

The Event schema only grows: existing fields never change meaning or type.
Unknown fields in incoming JSON are ignored. New `kind` values may appear in
minor versions; consumers should treat unknown kinds as `info`.
