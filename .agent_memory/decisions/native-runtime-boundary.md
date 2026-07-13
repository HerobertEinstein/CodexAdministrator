---
type: decision
status: superseded
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - docs/RUNTIME_ADAPTERS.md
  - src/protocol/
  - src/runtime_client.rs
verified_by:
  - historical repository state before Grok CLI/ACP removal
---
# Superseded Grok CLI And ACP Runtime Boundary

## Summary

The project previously explored separate child-process runtimes: Codex over
app-server JSONL and Grok CLI/Grok Build over ACP v1 JSON-RPC. That design made
Grok a separate main-agent route with its own session and permission protocol.

The route was abandoned for security reasons. It is not an active adapter,
fallback, compatibility target, or support claim. Current architecture uses the
official ChatGPT/Codex host as the sole agent runtime and limits Grok to a
Responses-compatible model provider.

## Evidence

- Grok protocol/client/process support is being removed from `src/` and tests.
- `docs/ARCHITECTURE.md` and `docs/RUNTIME_ADAPTERS.md` define the replacement
  native-provider boundary.

## Use Next Time

Do not restore Grok CLI, Grok Build, ACP, TUI parsing, or another Grok-owned
tool/approval/session loop as a fallback. Historical details may be retained
only when clearly marked superseded.

## Related / Supersedes

Superseded by
[Native Host And Grok Model Provider Boundary](native-model-provider-boundary.md).
