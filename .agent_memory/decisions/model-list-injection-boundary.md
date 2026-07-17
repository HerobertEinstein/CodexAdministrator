---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-17
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
identified Grok task, receives `modelProvider: "grok_native"`. The routable set
starts empty and is populated only for descriptors actually appended to a
matched native `model/list` response. An identical bootstrap reapplication
preserves the already reviewed routable set; only an actual configuration
change clears it. A configured ID that collides with a native entry therefore
never enters routing. GPT traffic, turn traffic, and existing native tasks
remain unchanged.

A remembered or imported Grok thread remains fail-closed while the current
routable model set is empty. Only after a matched native `model/list` response
confirms at least one reviewed descriptor may a model-less resume use its
remembered reviewed model or the first currently routable reviewed model.

Renderer reinjection may reuse only a byte-equivalent configuration carrying
the same private control nonce. The public namespaced `configure` method cannot
change descriptors, provider identity, or management-only state; any real
configuration change requires a fresh supervisor generation and Rust
validation.

The official host continues to own its interface, tools, approvals, sandbox,
workspace, task storage, and lifecycle. Model visibility is not evidence for
any additional capability. Optional native-state import is governed separately
by `native-state-import-boundary.md`; it creates private copies and never mutates
the daily native tasks referenced here.

Provider management belongs inside the native Codex model selector. Base URL,
API-key entry, live `/v1/models` refresh, model search, and the checkmarked set
of injected Grok IDs must be rendered as an additive selector panel in the
isolated official host. A separate Codex Administrator configuration window is
not an accepted product surface. The project-owned Windows process may remain
only as a headless lifecycle owner and secure credential broker; it must not
replace the official UI or attach to the daily instance.

Version 1 exposes Grok only. The discovery/cache/selector protocol should keep
provider-neutral data shapes where that does not weaken validation, so a later
version can support all providers, but the current injected UI and routing set
must filter to the exact case-sensitive reviewed registry: `grok-4.5`,
`grok-4.3-{low,medium,high}`, and
`grok-4.20-multi-agent-{low,medium,high,xhigh}`. Other Grok IDs and non-Grok
entries returned by `/v1/models` remain hidden and receive no inferred
capabilities. Registry admission does not prove current endpoint availability
or capability parity.
Native catalog entries use a fixed 32,768-token client compaction cap rather
than inheriting a GPT context limit. It is not an official provider maximum.
Reasoning summaries, search-tool support, and parallel tools remain disabled;
exact shell/tool evidence remains model-specific.

## Evidence

- Message-level tests cover native-list preservation, deduplication, all known
  task-start shapes, GPT object identity, task learning, model-less resume,
  delayed bridge initialization, and disposal.
- Provider tests cover atomic writes, unrelated-field preservation,
  idempotence, endpoint validation, and secret non-persistence.
- Production Direct desktop E2E now proves native-list preservation, one Grok
  entry, native-menu Grok selection, GPT-5.4 restoration, renderer-reload
  reinjection, and exact cleanup. Endpoint capability parity remains unproven.
- A retained-profile E2E caught and fixed an identical-reconfiguration reset:
  after reapplication, a new turn stored `modelProvider = grok_native`,
  `model = grok-4.5`, and `effort = high` instead of falling back to `openai`.
- The dated 2026-07-17 r26 run, before the exact reviewed-registry filter was added,
  observed 12 upstream Grok IDs and proved a real native Advanced -> Model
  interaction, native keyboard selection of `grok-4.5`, one exact nonce
  response, and an official rollout whose `session_meta.model_provider` is
  `grok_native` and whose `turn_context.model` is `grok-4.5`. It is historical
  evidence for that exact model and route, not evidence that all 12 IDs remain
  injectable. Publication remains governed by the repository freshness gate:
  after any later source or public-document change, rerun the complete locked
  gates before pushing.

## Use Next Time

Do not add another interface, execution loop, global model selection, model
catalog replacement, or provider switch on turn requests. Do not describe the
integration as complete native support until capability-specific desktop E2E
evidence exists. Do not infer parallel-tool or multi-call parity from renderer
metadata or a single successful tool loop; transient endpoint outcomes belong
in dated local evidence rather than this active decision.
Do not restore the removed standalone configuration GUI; configuration and
catalog management must stay inside the native model selector.
Do not broaden the reviewed registry from an ID prefix, dynamic discovery
result, or display name alone. Add an exact capability profile, tests, public
documentation, and model-specific evidence before admitting another ID.
