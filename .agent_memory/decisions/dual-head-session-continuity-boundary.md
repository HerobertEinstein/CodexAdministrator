---
type: decision
status: active
created: 2026-07-18
updated: 2026-07-18
scope: project
paths:
  - src/native_session_continuity.rs
  - src/native_state_sync.rs
  - src/windows_runtime.rs
  - tests/native_session_continuity.rs
verified_by:
  - official Codex app-server 0.142.3 experimental protocol schema
  - D:/VSC/work/HE BOX/_runs/current/evidence/codex-administrator-dual-head-session-audit-20260718.md
---
# Dual-Head Session Continuity Boundary

## Summary

One-way rollout import provides task visibility, not lossless collaboration.
Every shared logical task must retain an independent daily head and isolated
head. Each head records the latest turn id and status, stable item ids or a
content fingerprint, provider, update time, and whether a turn is in progress.
The continuity record also stores the newest exact common completed turn.

The main branch has one active writer at a time. If both lanes advance from the
same common point, neither lane may overwrite the other. Both heads remain as
explicit branches until a mediated handoff or merge creates a new continuation.
An interrupted snapshot is not silently treated as a completed remote turn.

## Official Boundary

Use official app-server semantics for observation and handoff. The currently
audited protocol exposes `thread/read`, `thread/turns/list`,
`thread/turns/items/list`, `thread/fork`, `thread/rollback`, and
`thread/inject_items`, plus `thread/status/changed`, `turn/started`, and
`turn/completed` notifications. Raw rollout or SQLite mirroring is not the
coordination mechanism.

The current deployment does not yet satisfy this boundary. Its session import
runs at managed launch and the isolated copy can remain behind while the daily
task advances. Do not describe current task import as real-time or lossless.

The current uncommitted source foundation now includes exact-head comparison,
an atomic compact continuity manifest, official read/turn RPC head collection,
and an event-driven Windows session-directory monitor. A real two-home Rust
probe passed. The observer is not wired into the managed launcher or deployed.
An independent app-server received zero notifications while the daily desktop
advanced, so cross-process notifications cannot replace the directory trigger.

## Use Next Time

Implement head comparison and durable conflict state before adding any write or
handoff path. A same-turn id with different item fingerprints is divergence,
not equality. Keep the daily home read-only unless an official app-server write
operation is explicitly selected and verified. Never resolve concurrent changes
with last-writer-wins behavior. Before making both agents consume the state,
preserve the no-touch daily boundary unless the owner explicitly accepts an
official model-visible history injection.
