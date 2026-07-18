---
type: decision
status: active
created: 2026-07-18
updated: 2026-07-18
scope: project
paths:
  - compatibility.json
  - src/compatibility.rs
  - src/host.rs
  - src/startup.rs
  - docs/COMPATIBILITY.md
  - docs/HOST_ADAPTERS.md
verified_by:
  - official Codex++ v1.2.34, v1.2.35, and main source comparison
  - GitHub pull request 719 and 350 metadata and patches
  - cargo test --test compatibility_policy --test startup_contract --locked
---
# Codex++ Isolated Owner Compatibility Contract

## Summary

A Codex++ executable hash is necessary but never sufficient to enable the
adapter. Schema 2 requires
`composition_contract=isolated_codex_plus_owner_v1`, and the programmatic policy
API rejects hash-only Codex++ approval.

The contract represents retained E2E proof that Codex++ owns one disjoint
official host with a separate profile, `CODEX_HOME`, state/SQLite, process tree,
guard/CDP/helper ports, and no activation, restart, termination, or write against
the daily instance. Administrator may then use only Codex++'s external
`user_scripts` slot.

## Evidence

Codex++ `v1.2.34`, `v1.2.35`, and current `main` resolve
`.codex-session-delete` through Windows Known Folder profile state, so changing
`APPDATA` or `USERPROFILE` does not isolate settings and status. The current
owner settings also contain no `--user-data-dir` argument.

Open PR `#719` adds per-instance ports and `--user-data-dir`, but it does not
isolate Codex++ app state or `CODEX_HOME`. Open PR `#350` adds direct Windows
launch by first stopping existing Codex processes, which violates the daily
instance boundary. Neither PR satisfies this contract.

## Use Next Time

Do not add an installed or official-release hash to `compatibility.json` merely
because the binary matches GitHub assets or exposes `user_scripts`. Require the
full contract and immutable E2E evidence in the same publication. Until then,
keep `found=true`, `eligible=false`, remove only stale Administrator-owned
script residue, and do not launch Codex++.
