# Example: wiring opencode to herald

opencode integrates through in-process JS/TS plugins rather than exec hooks —
which is exactly what `herald emit --json` exists for: any runtime that can
spawn a process can notify. Drop this in your opencode plugin directory
(e.g. `.opencode/plugin/herald.ts`) and adjust event names to the plugin API
version you're running:

```ts
// .opencode/plugin/herald.ts — forward opencode events to herald
import { spawn } from "node:child_process"

function emit(event: Record<string, unknown>) {
  const child = spawn("herald", ["emit", "--json"], { stdio: ["pipe", "ignore", "ignore"] })
  child.stdin.end(JSON.stringify(event))
}

export const HeraldPlugin = async ({ project }: { project: { path?: string } }) => ({
  event: async ({ event }: { event: { type: string; properties?: any } }) => {
    switch (event.type) {
      case "session.idle": // turn finished, agent waiting
        emit({
          source: "opencode",
          kind: "turn-complete",
          body: "Session idle — turn finished",
          cwd: project?.path,
        })
        break
      case "permission.updated": // agent blocked on an approval
        emit({
          source: "opencode",
          kind: "attention",
          body: "opencode is waiting for your approval",
          cwd: project?.path,
        })
        break
      case "session.error":
        emit({
          source: "opencode",
          kind: "error",
          body: String(event.properties?.error ?? "session error"),
          cwd: project?.path,
        })
        break
    }
  },
})
```

That's the whole integration: ~30 lines, no herald-side changes, and the
events inherit everything — focus-aware routing, quiet hours, burst
coalescing, muxer sinks, the log. This is why herald ships no plugin API:
the canonical Event **is** the API (see [CONTRACT.md](../CONTRACT.md)).
