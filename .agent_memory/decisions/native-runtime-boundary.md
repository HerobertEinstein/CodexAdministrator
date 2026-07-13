---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - src/jsonl.rs
  - src/runtime_process.rs
  - src/runtime_client.rs
  - src/protocol/
  - docs/RUNTIME_ADAPTERS.md
verified_by:
  - cargo test --all-targets
  - cargo check --all-targets
  - cargo test --test codex_live -- --ignored --nocapture
---
# Native Runtime Process And Protocol Boundary

## Summary

Grok and Codex use their own native protocols over direct child-process JSONL.
Codex uses app-server messages without a `jsonrpc` member. Grok uses ACP v1
JSON-RPC 2.0. Shared code owns only framing, request correlation, stderr
separation, timeouts, and lifecycle containment.

On Windows, runtime process trees are assigned to a kill-on-close Job Object.
Shell wrappers are never executed. The official npm Codex installation is
resolved to `node.exe` plus the absolute `@openai/codex/bin/codex.js` path;
WindowsApps desktop-package resources are not treated as generic CLI entries.

## Evidence

Unit and integration tests cover out-of-order string/numeric IDs, notifications,
server requests, timeout cleanup, protocol message shapes, initialization order,
stderr separation, and Job Object execution. The environment-gated live Codex
test completed official `initialize -> initialized -> thread/start` on Windows.

## Use Next Time

Do not merge the Codex and Grok wire formats. Do not invoke `.cmd`, PowerShell,
TUI parsing, or `codex exec` as the primary parity path. Add real turn, approval,
resume, cancellation, and authenticated Grok E2E evidence before claiming those
capabilities complete.
