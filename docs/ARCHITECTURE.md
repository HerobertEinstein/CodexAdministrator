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

The routable Grok set starts empty and is populated only by descriptors
actually appended to that matched response. Renderer reconfiguration clears it
back to fail-closed until another response confirms the append. A configured
ID that collides with a native entry therefore never becomes routable.

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

The bootstrap locates the official same-origin `vscode-api-*` renderer module
from the native entry bundle and wraps its writable `postMessage` abstraction.
It does not replace the frozen `window.electronBridge` object or its native
method. A writable legacy bridge is a compatibility fallback only. Disposal
restores the exact prior renderer function only when the wrapper still owns
that slot, removes its capture listener, stops retries, and clears
project-owned task state.

## Host Adapters

The bridge source is host-independent. Adapters only determine how the same
script reaches the page:

- `direct`: reserved for an isolated official desktop instance and currently
  implemented with separate profile, `CODEX_HOME`, suspended-process package
  verification, Windows Job Object ownership, listener-PID-bound loopback CDP,
  UI readiness, bounded reload recovery, and exact cleanup;
- `codexplusplus`: external user script, enabled only for an exact reviewed
  executable identity.

No adapter may modify an official installation file.

## Direct Instance Isolation

The direct adapter may never attach to, activate, restart, close, or inject the
currently used daily ChatGPT/Codex instance. Its launch contract requires:

- a project-owned profile that does not overlap the daily profile;
- a project-owned `CODEX_HOME` that does not overlap any daily path;
- an instance path whose existing ancestors contain no reparse point;
- a new process tree disjoint from every pre-existing ChatGPT PID;
- a new loopback CDP port whose listener PID belongs to that process tree, with
  one `app://-/index.html` renderer target; and
- continuous proof that the daily root instance remains alive.

Windows currently needs a two-stage launch for the isolated profile: the first
start creates its background process and CDP endpoint, and a second start with
the same isolated arguments plus `--new-window` creates that isolated
renderer. `CreateProcessW` starts each root process suspended, verifies its
actual image and `OpenAI.Codex_2p2nqsd0c76g0` package family, assigns it to the
project Job Object, and only then resumes it. Failure at any security gate
terminates that Job Object and removes only the exact project-owned instance
root.

The launcher snapshots every pre-existing ChatGPT PID before startup and
requires those PIDs to remain alive. It accepts only one
`app://-/index.html` target from its own loopback port. The CDP client validates
every command response, waits for bridge health and native UI readiness, and
reinstalls the same idempotent bootstrap when a renderer reload clears it.
Missing targets and renderer-health disconnects receive only a bounded recovery
window; package, PID, listener ownership, and target-identity failures remain
immediate fail-closed errors.

The launcher proves that it passed the isolated profile argument and the
current package E2E proves that this release honored it. It does not fabricate a
runtime profile observation by copying the contract value back into its own
verification input.

The provider route does not require or launch a separate Grok desktop client or
execution host.

## Capability Boundary

Seeing and selecting a model proves only model-list and routing behavior.
Streaming, tools, files, images, structured output, reasoning controls,
cancellation, and reliable resume are independent evidence gates. The project
must not advertise any of them from model visibility alone.
