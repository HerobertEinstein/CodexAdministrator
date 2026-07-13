# Native Runtime Adapters

## Grok Build

The primary Grok adapter launches the official Windows executable directly with:

```text
grok.exe agent --no-leader stdio
```

The transport is ACP protocol version 1 over JSON lines on stdin/stdout. It supports `session/new`, `session/load`, `session/prompt`, and `session/cancel`, plus structured message, thought, tool, and plan updates.

The adapter does not parse the TUI, invoke a shell, use `npx`, parse human `sessions list` output, or redistribute the proprietary Grok binary. Installation must use the official npm registry and independently verify the package integrity and reported agent version.

On Windows the Grok process tree must run in a Job Object. Cancellation first sends `session/cancel`, then closes stdin and terminates the Job Object after a bounded timeout. Cancellation does not imply filesystem rollback.

## Codex

The primary Codex adapter launches the official Windows executable directly with:

```text
codex.exe app-server --stdio
```

The transport is the official bidirectional JSONL app-server protocol. The client sends `initialize`, waits for its response, sends `initialized`, and then uses `thread/start` or `thread/resume`, `turn/start`, and `turn/interrupt`.

The companion preserves `thread.id`, `thread.sessionId`, and `turn.id` as distinct identities. Approval requests are answered with the original JSON-RPC request id. The experimental WebSocket transport and Unix-only daemon lifecycle are not used on Windows.

`codex exec --json` is a fallback for bounded non-interactive work only. It cannot provide full approval, user-input, or dynamic tool behavior and is not the native parity transport.

## Shared rules

- Each native runtime owns its prompts, tools, approvals, session history, compaction, credentials, and errors.
- The companion normalizes lifecycle events for display but does not replay one runtime's tool calls through another.
- Exact executable and protocol versions are part of the compatibility identity.
- Unknown events are preserved and ignored safely until their capability is understood.
- Runtime stdout is protocol-only; stderr is diagnostic-only and secrets are redacted.
