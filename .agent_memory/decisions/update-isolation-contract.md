---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - src/host.rs
  - src/compatibility.rs
  - docs/COMPATIBILITY.md
  - docs/HOST_ADAPTERS.md
  - docs/UPDATE_ISOLATION.md
verified_by:
  - cargo test --test codex_plus_adapter --test compatibility_policy --test startup_contract
  - cargo test --test repository_contract
---
# Update Isolation Contract

## Summary

Official ChatGPT/Codex and Codex++ installations and update mechanisms remain
publisher-owned. The project may write only the exact Grok provider entry and,
for a verified Codex++ executable, its exact external script file and enablement
key.

Unknown host identities fail closed. They do not block the host or alter native
GPT behavior. Provider registration preserves model selection and unrelated
configuration.

## Use Next Time

Never modify installation files, packaged resources, signatures, profiles,
updater state, or native model defaults. Renew compatibility evidence after a
host update instead of pinning or interfering with that update.
