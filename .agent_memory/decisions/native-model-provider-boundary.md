---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - src/native_provider.rs
  - docs/ARCHITECTURE.md
  - docs/RUNTIME_ADAPTERS.md
  - docs/COMPATIBILITY.md
  - docs/UPDATE_ISOLATION.md
verified_by:
  - cargo test --test native_model_provider
  - cargo test --test codex_live -- --ignored --nocapture --test-threads=1
  - scoped stale-claim scan of docs and .agent_memory on 2026-07-13
---
# Native Host And Grok Model Provider Boundary

## Summary

The official ChatGPT/Codex host is the only agent runtime and owns the agent
loop, tools, approvals, sandbox, workspace, sessions, compaction, and
cancellation. Grok is only a Responses-compatible model provider configured
through the supported user configuration surface.

Provider configuration stores an environment-variable name, never a secret
value. Remote endpoints require HTTPS, loopback development may use HTTP, and
the base URL must end in `/v1`. Provider registration preserves unrelated
configuration. Explicit launch selects Grok through the supported persisted
model/provider fields and keeps a fail-closed backup for `launch-native`.

Model visibility is not capability parity. Every model/provider capability
requires separate official-host E2E evidence. Unknown behavior fails closed
without a Grok CLI/ACP fallback and without weakening host-owned controls.

## Evidence

- `src/native_provider.rs` writes a `wire_api = "responses"` provider using an
  `env_key` reference and validates endpoint and environment-key syntax.
- The native launcher persists the supported provider/model selection and then
  invokes the official `codex app <workspace>` path without ineffective `-c`
  arguments or secret-bearing arguments.
- A credential-free sidecar preserves the previous model/provider/catalog
  selection; restoration refuses to overwrite later user changes.
- Reviewed model metadata is passed only through Codex's official
  `model_catalog_json` surface after required-shape and exact selected-slug
  validation plus an installed-runtime `debug models` parse gate.
- `NativeProviderCapabilityManifest` binds exact models and explicit,
  default-disabled capabilities to an immutable evidence digest.
- `tests/native_model_provider.rs` covers secret non-persistence, default and
  future-field preservation, idempotence, HTTPS/loopback rules, invalid
  environment keys, native app launch arguments, and capability defaults.
- `tests/codex_live.rs` proves custom-provider thread selection, SSE text
  deltas, a host-owned function-tool round trip, and local PNG conversion using
  installed official Codex 0.142.3 with isolated configuration.
- The architecture and compatibility docs define the host/provider ownership
  and fail-closed capability boundary.

## Use Next Time

Do not describe Grok as owning tools, approvals, sandboxing, workspaces,
sessions, or a main-agent process. Do not infer support from model discovery.
Reject or withhold unsupported capabilities while keeping the official host
and existing providers operational.

## Related / Supersedes

Supersedes
[Superseded Grok CLI And ACP Runtime Boundary](native-runtime-boundary.md).
