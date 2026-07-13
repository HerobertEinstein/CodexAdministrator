# Native Runtime Adapters

## Grok CLI And Grok Build

The native runtime is the official `@xai-official/grok` package and its `grok`
executable. Grok Build is a coding-agent/model profile available through that
CLI, not a second executable or transport. The adapter launches:

```text
grok.exe agent --no-leader stdio
```

The transport is ACP protocol version 1 over JSON lines on stdin/stdout. It supports `session/new`, `session/load`, `session/prompt`, and `session/cancel`, plus structured message, thought, tool, and plan updates.

`grok models` may expose profiles other than `grok-build`. Discovery proves only
that the CLI lists an identifier. Each profile still needs its own capability
probe and E2E evidence before it inherits Grok Build tool, permission, session,
or coding-environment claims.

The adapter does not parse the TUI, invoke a shell, use `npx`, parse human `sessions list` output, or redistribute the proprietary Grok binary. Installation must use the official npm registry and independently verify the package integrity and reported agent version.

The official `xai-org/grok-build-plugin-cc` project is a Claude Code bridge that
invokes the same `grok` CLI. It is not an alternative Grok runtime or GUI host.

On Windows the Grok process tree must run in a Job Object. Cancellation first sends `session/cancel`, then closes stdin and terminates the Job Object after a bounded timeout. Cancellation does not imply filesystem rollback.

## Codex

The adapter resolves the installed official Codex CLI without invoking a shell.
For a standalone executable it launches:

```text
codex.exe app-server --stdio
```

For the official npm installation it resolves the wrapper to its owned runtime
and launches the equivalent command directly:

```text
node.exe <absolute-path-to-@openai/codex/bin/codex.js> app-server --stdio
```

It never executes `codex.cmd`, PowerShell wrappers, or the ACL-protected
`WindowsApps` desktop-package resource as a generic CLI entry.

The transport is the official bidirectional JSONL app-server protocol. The client sends `initialize`, waits for its response, sends `initialized`, and then uses `thread/start` or `thread/resume`, `turn/start`, and `turn/interrupt`.

The companion preserves thread and turn identities separately. Approval requests
are answered with the original server-request id. Codex app-server messages do
not add a `jsonrpc` field. The experimental WebSocket transport and Unix-only
daemon lifecycle are not used on Windows.

`codex exec --json` is a fallback for bounded non-interactive work only. It cannot provide full approval, user-input, or dynamic tool behavior and is not the native parity transport.

## Shared rules

- Each native runtime owns its prompts, tools, approvals, session history, compaction, credentials, and errors.
- The companion normalizes lifecycle events for display but does not replay one runtime's tool calls through another.
- Exact executable and protocol versions are part of the compatibility identity.
- Unknown events are preserved and ignored safely until their capability is understood.
- Runtime stdout is protocol-only; stderr is diagnostic-only and secrets are redacted.
- Every Windows runtime is assigned to a Job Object with kill-on-close so the
  launcher cannot leave a detached process tree after exit.

## Current proof

The repository has deterministic protocol/transport tests for both runtimes and
a Windows environment-gated Codex test that has completed a real official
`initialize -> initialized -> thread/start` sequence. Real Codex turns, approval
UI, resume behavior, authenticated Grok sessions, and Grok permission prompts
still require their separate E2E gates before parity is claimed.
