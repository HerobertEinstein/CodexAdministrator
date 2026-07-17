---
type: decision
status: active
created: 2026-07-18
updated: 2026-07-18
scope: project
paths:
  - src/native_skill_sync.rs
  - src/launcher_settings.rs
  - src/launcher.rs
  - src/main.rs
  - src/windows_runtime.rs
  - assets/model-picker-mount.js
  - tests/native_skill_sync.rs
verified_by:
  - cargo test --all-targets --locked
  - cargo clippy --all-targets --all-features --locked -- -D warnings
  - node --test tests/*.test.mjs
  - D:/VSC/work/HE BOX/_runs/current/evidence/codex-administrator-managed-skills-sync-deployment-20260718.md
---
# Custom Skill Projection Boundary

## Summary

Managed launches project custom entries from the daily `CODEX_HOME/skills`
into the retained isolated home by default. The daily directory is the only
canonical source. The official `.system` tree, caches, temporary residue,
reparse points, symbolic links, junctions, and hard-linked inputs are excluded.
Daily and isolated `CODEX_HOME` databases are never mirrored.

`skill-projection-manifest.json` records source and destination SHA-256 values
for every project-owned file. A destination may be updated or removed only
while it still matches the prior projection. Missing, modified, hard-linked,
reparse-backed, and unmanaged isolated paths are preserved as review
conflicts. No isolated change is automatically written back to daily Skills.

## Evidence

Commit `519bd57` implements the projector, setting, hidden CLI argument,
manager toggle, runtime wiring, public docs, and 13 Windows behavior tests.
Commit `99b6a4a` records the current owner deployment. Two live managed starts
projected 99 files with zero conflicts and identical second-launch hashes while
preserving the official `.system` tree, 332 task snapshots, launcher settings,
and all nine daily ChatGPT process identities.

## Use Next Time

Keep Skill sharing one-way and manifest-owned. Do not copy `.system`, blindly
mirror directories, adopt an unmanaged destination, or write isolated edits
back to the daily source. Preferences, project/global memory, evolution, and
Goal continuity still need a separate canonical mediated-write layer rather
than file or SQLite mirroring.
