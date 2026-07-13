# Architecture

## Canonical Topology

The official ChatGPT/Codex application remains the only user interface and
execution host. Codex Administrator adds model metadata and provider routing
at the host message boundary; it does not own prompts, tools, approvals,
sandbox policy, workspace access, task storage, compaction, or cancellation.

```text
user configuration
  `-- [model_providers.grok_native]

native host message bridge
  +-- model/list response: preserve every native entry, append Grok once
  +-- Grok thread/start: add modelProvider = grok_native
  +-- known Grok thread/resume: add modelProvider = grok_native
  `-- all other messages: return the original object unchanged
```

## Provider Registration

`configure-provider` atomically creates or updates only
`model_providers.grok_native`. It preserves native model selection, unrelated
providers, unknown future fields, and all other user configuration.

The provider uses the Responses wire API. Configuration stores an uppercase
environment-variable name, not the credential value. Remote endpoints require
HTTPS, while loopback HTTP is accepted for tests. The base URL must end in
`/v1` and may not contain credentials, a query, or a fragment.

## Model Descriptors

Injected descriptors are separate from provider configuration. They are
validated, deduplicated, and appended only to a response belonging to a
tracked `model/list` request. Existing native entries retain their original
identity, order, fields, and object references.

The current generated descriptor uses conservative text-only transport
metadata so the native selector has the required shape. That metadata is not a
claim that any endpoint or model has passed reasoning or modality E2E.

A descriptor intentionally contains no provider field because the native
model-list schema does not carry provider identity. Provider routing occurs on
task creation or resume, where the official protocol exposes
`modelProvider`.

## Task Routing

The bridge writes `modelProvider: "grok_native"` only for these cases:

- a new task explicitly selects an injected Grok model;
- a task response identifies its provider as `grok_native`, after which a
  model-less resume for that exact task is routed to the same provider.

It does not rewrite turn requests or attempt to move an existing native GPT
task between providers. GPT traffic is passed through as the same object.

## Lifecycle

The bootstrap waits for a delayed native bridge, installs one wrapper, and
exposes a namespaced health/dispose handle. Disposal restores the exact
official function only when the wrapper still owns that slot, removes its
capture listener, stops retries, and clears project-owned task state.

## Host Adapters

The bridge source is host-independent. Adapters only determine how the same
script reaches the page:

- `direct`: reserved for a project-owned desktop debugging connection and
  currently disabled pending desktop E2E;
- `codexplusplus`: external user script, enabled only for an exact reviewed
  executable identity.

No adapter may modify an official installation file.

## Capability Boundary

Seeing and selecting a model proves only model-list and routing behavior.
Streaming, tools, files, images, structured output, reasoning controls,
cancellation, and reliable resume are independent evidence gates. The project
must not advertise any of them from model visibility alone.
