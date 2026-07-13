---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - assets/bootstrap.js
  - assets/model-injection-core.js
  - src/bootstrap.rs
  - src/native_provider.rs
  - docs/ARCHITECTURE.md
verified_by:
  - node --test tests/model_injection.test.mjs tests/bootstrap_runtime.test.mjs
  - cargo test --test bootstrap_contract --test native_model_provider
---
# Model-List Injection Boundary

## Summary

The only integration target is the native ChatGPT/Codex model list. Provider
registration writes only `model_providers.grok_native`. The message bridge
preserves every native model and appends Grok descriptors once.

Only a new task selecting Grok, or a model-less resume for an already
identified Grok task, receives `modelProvider: "grok_native"`. GPT traffic,
turn traffic, and existing native tasks remain unchanged.

The official host continues to own its interface, tools, approvals, sandbox,
workspace, task storage, and lifecycle. Model visibility is not evidence for
any additional capability.

## Evidence

- Message-level tests cover native-list preservation, deduplication, all known
  task-start shapes, GPT object identity, task learning, model-less resume,
  delayed bridge initialization, and disposal.
- Provider tests cover atomic writes, unrelated-field preservation,
  idempotence, endpoint validation, and secret non-persistence.
- The direct official desktop adapter remains disabled until real desktop E2E
  evidence exists.

## Use Next Time

Do not add another interface, execution loop, global model selection, model
catalog replacement, or provider switch on turn requests. Do not describe the
integration as complete native support until capability-specific desktop E2E
evidence exists.
