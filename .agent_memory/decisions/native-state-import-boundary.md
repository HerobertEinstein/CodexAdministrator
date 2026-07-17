---
type: decision
status: active
created: 2026-07-16
updated: 2026-07-18
scope: project
paths:
  - src/native_state_sync.rs
  - src/launcher_settings.rs
  - src/bin/codex-administrator-launcher.rs
  - tests/native_session_sync.rs
  - tests/launcher_settings.rs
verified_by:
  - cargo test --test native_session_sync
  - cargo test --test launcher_settings
---
# Native State Import Boundary

## Summary

The native model-selector manager synchronizes validated file-backed
`auth.json` by default. Managed launches also enable one-way incremental
conversation import by default, with a visible opt-out, because manual import
does not satisfy seamless task continuity. The import copies prompts, messages,
tool output, and environment history, can require several GB, and may send that
history to the selected Grok provider when a thread is continued.

The daily read allowlist is exact: `auth.json`, `sessions/**/*.jsonl`,
`archived_sessions/**/*.jsonl`, and `session_index.jsonl`. Never read or copy
daily `config.toml`, SQLite/WAL/SHM, logs, goals, memories, hard links,
junctions, or symbolic links. Never write imported state back to daily paths.
Both the auth source and each rollout must have a single hard-link count;
shared hard-linked inputs are rejected before publication into isolated state.
Every daily auth/session source is opened with
`FILE_FLAG_OPEN_REPARSE_POINT` and validated from that handle, so a final file
link or a post-enumeration link replacement is never followed outside the daily
root.

## Import Contract

Rollouts use non-blocking shared reads so the daily instance is not held for a
large copy. Publication requires stable file identity, size, modification time,
complete JSONL, a full hash, and a second consistency pass. The isolated copy
rewrites only canonical `session_meta.payload.model_provider` to `grok_native`.
This makes the thread visible and routable; it does not prove reliable resume or
capability parity for arbitrary GPT histories.

The per-file manifest verifies both source and destination hashes, recovers a
fully published update after an interrupted manifest write, and keeps one
private active-or-archived copy per thread. A locally changed private rollout
wins and is never overwritten. `session_index.jsonl` follows append-only
last-entry semantics within each file; between daily and private state, the
newer parsed RFC3339 name wins and ties remain private.

## Use Next Time

Keep complete conversation import enabled by default for managed launches while
retaining the explicit privacy/space/provider warning and a visible opt-out.
Synchronization remains one-way and incremental at launch. Do not broaden the
daily read allowlist, share SQLite, copy daily configuration, or advertise
resume parity without new tests and a real official app-server list/read/resume
E2E.
