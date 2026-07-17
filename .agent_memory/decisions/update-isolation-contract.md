---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-17
scope: project
paths:
  - src/host.rs
  - src/isolation.rs
  - src/compatibility.rs
  - src/launcher_settings.rs
  - src/credential_store.rs
  - src/native_state_sync.rs
  - src/renderer_addons.rs
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
publisher-owned. Project-owned state is limited to the exact Grok provider
entry, non-secret launcher settings, one Windows Generic Credential, an
isolated profile and `CODEX_HOME`, validated one-way native auth/task copies,
renderer-addon settings, and, for a verified Codex++ executable, its exact
external script file and enablement key. No owned path may overlap a daily path,
and external addon checkouts remain read-only.

Unknown Codex++ executable identities fail closed. Direct instead requires the
protected official package path, suspended image and package-family identity,
Job/listener ownership, and live target/bridge/UI gates. Failures do not block
the publisher host or alter native GPT behavior. Provider registration
preserves model selection, existing tool configuration, existing providers,
and unrelated or unknown configuration. Its shell-policy merge only adds the
provider secret to the exclusion list and rejects explicit secret
reintroduction. Stored and pending credentials are bound to the Base URL plus
effective Action Path they verified; the Credential Manager blob contains that
endpoint fingerprint, so offline settings replacement cannot reuse the key
after restart. Both launcher stages scrub secret-shaped inherited environment
variables, and management-only explicitly blocks its configured provider env
key. Provider cleanup retains shell exclusions whose prior ownership cannot be
proved instead of deleting a possibly user-owned rule.

## Use Next Time

Never modify installation files, packaged resources, signatures, the daily
profile, daily auth/tasks, updater state, native model defaults, or external
addon checkout. A project-owned isolated profile must pass the path-overlap and
runtime ownership contract before use. Renew compatibility evidence after a
host update instead of pinning or interfering with that update.
