---
type: decision
status: active
created: 2026-07-18
updated: 2026-07-18
scope: project
paths:
  - src/native_goal_sync.rs
  - src/native_state_sync.rs
  - src/launcher_settings.rs
  - src/windows_runtime.rs
  - assets/model-picker-mount.js
  - tests/native_goal_sync.rs
verified_by:
  - cargo test --all-targets --locked
  - node --test tests/*.test.mjs
  - D:/VSC/work/HE BOX/_runs/current/evidence/codex-administrator-goal-sync-owner-deployment-20260718.md
---
# Goal Intent Sync Boundary

## Summary

Optional Goal intent synchronization is disabled by default and requires native
task synchronization. It discovers the native binary from an installed
official npm Codex package, starts short-lived app-server helpers against the
daily and isolated homes with plugins and apps disabled, and calls only
`thread/goal/get`, `thread/goal/set`, and `thread/goal/clear`.

The isolated `goal-intent-sync-manifest.json` stores the last common intent for
thread IDs in the merged session index. One-sided changes propagate in either
direction. Different changes on both sides, or a destination changed during the
pre-write re-read, remain conflicts and are not overwritten. Only objective,
status, and optional token budget are synchronized. Official token and elapsed
time counters remain local to each app-server because the write API does not
accept imported counters.

## Evidence

The pure merge tests cover both directions, clear propagation, divergent
changes, and concurrent destination changes. The JSON-line protocol tests cover
official initialize identity, CODEX_HOME binding, Goal get/set/clear shapes,
ignored usage counters, and two disjoint spawned app-server homes. Full Rust and
Node suites pass. The current owner deployment explicitly opted in through the
native manager and completed two managed restarts. Its settings and manifest
hashes remained stable, the manifest held 14 currently available shared records
with zero conflicts, and official RPC reads returned the same intent hash from
the daily and isolated homes.

## Use Next Time

Do not copy `goals_*.sqlite`, WAL, SHM, or usage counters. Do not enable Goal
sync without task sync or an accessible official npm Codex CLI. Treat the npm
Codex installation and explicit helper override as trusted local inputs. Keep
the write-before-read gate and durable conflict record; do not replace them
with last-writer-wins behavior. Keep the product default disabled even though
the current owner deployment has explicitly enabled it.
