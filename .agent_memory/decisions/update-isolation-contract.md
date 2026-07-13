---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-14
scope: project
paths:
  - src/host.rs
  - src/isolation.rs
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
key. A direct adapter may create only a project-owned isolated profile and
isolated `CODEX_HOME`; neither may overlap a daily path.

Unknown Codex++ executable identities fail closed. Direct instead requires the
protected official package path, suspended image and package-family identity,
Job/listener ownership, and live target/bridge/UI gates. Failures do not block
the publisher host or alter native GPT behavior. Provider registration
preserves model selection and unrelated configuration.

## Use Next Time

Never modify installation files, packaged resources, signatures, the daily
profile, updater state, or native model defaults. A project-owned isolated
profile must pass the path-overlap and runtime ownership contract before use.
Renew compatibility evidence after a host update instead of pinning or
interfering with that update.
