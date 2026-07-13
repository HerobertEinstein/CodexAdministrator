---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - src/native_provider.rs
  - docs/UPDATE_ISOLATION.md
  - docs/COMPATIBILITY.md
verified_by:
  - cargo test --test native_model_provider
  - git diff --check -- docs .agent_memory
---
# Update Isolation And Provider Configuration Ownership

## Summary

Codex Administrator is external to official ChatGPT/Codex and Codex++
installations and update mechanisms. It must never modify, block, pin, spoof,
or roll back them. The active Grok integration is limited to a
Responses-compatible provider entry in supported user-owned configuration.

The entry contains an environment-variable reference only. Invalid provider
configuration fails closed and leaves official startup, existing providers,
and unrelated settings unchanged. Explicit Grok launch changes only the
supported selection fields and keeps their previous values for fail-closed
restoration.

## Evidence

- `docs/UPDATE_ISOLATION.md` defines installation, updater, configuration, and
  removal boundaries.
- `docs/COMPATIBILITY.md` defines fail-closed provider and capability gates.
- `tests/native_model_provider.rs` checks non-persistence of secrets,
  preservation of unrelated fields and defaults, idempotence, and validation.

## Use Next Time

1. Never write into official ChatGPT/Codex or Codex++ installation or updater
   state.
2. Store only environment-variable names, never credential values.
3. Preserve unrelated and future configuration fields; change model selection
   only for explicit launch and retain its exact restorable predecessor.
4. Fail closed at the provider or capability boundary without blocking the
   official host.
5. Remove only exact project-owned configuration and files.

Treat requests for Grok support as native-host model-provider work, not as
permission to add a Grok CLI/ACP agent runtime or patch an official product.

## Related / Supersedes

This entry replaces the earlier injection-focused update-isolation mechanism
while preserving the stronger invariant that official products and their
updaters remain untouched.
