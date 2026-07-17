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

The official authentication variable `OPENAI_API_KEY` is not a valid custom
provider `env_key` because the host may persist it under isolated
`CODEX_HOME/auth.json`. Provider-specific names remain environment-only and
avoid the official authentication cache path.

## Model Descriptors

Injected descriptors are separate from provider configuration. They are
validated, deduplicated, and appended only to a response belonging to a
tracked `model/list` request. Existing native entries retain their original
identity, order, fields, and object references.

The exact `grok-4.5` descriptor exposes the xAI-documented low, medium, and
high reasoning efforts; the default is high and reasoning cannot be disabled.
The reviewed sources are `https://docs.x.ai/developers/grok-4-5` and
`https://docs.x.ai/developers/model-capabilities/text/reasoning`.

Every injected native catalog entry uses a 32,768-token conservative client cap
for host compaction. The cap is not the provider's official maximum and is not
copied from the first GPT model. Reasoning summaries, search-tool support, and
parallel tool calls remain disabled. The required native schema still uses its
`shell_command` and text web-search shape, but exact tool evidence currently
covers only the separately documented `grok-4.5` loops.

The routable Grok set starts empty and is populated only by descriptors
actually appended to that matched response. Reapplying the byte-equivalent
bootstrap configuration preserves that reviewed set; a real configuration
change clears it back to fail-closed until another response confirms the
append. A configured ID that collides with a native entry therefore never
becomes routable.

The reviewed capability registry admits only the case-sensitive IDs `grok-4.5`,
`grok-4.3-{low,medium,high}`, and
`grok-4.20-multi-agent-{low,medium,high,xhigh}`. The exact `grok-4.5`
descriptor exposes the xAI-documented text and image input modalities; the
fixed-effort aliases expose text input and only the effort encoded in their
model ID. Unreviewed Grok IDs are not injected. Descriptor metadata gives the
native selector its required shape but is not a claim that any endpoint or
model has passed image or other modality E2E.

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
project-owned task state. It also asks the optional renderer-addon registry to
dispose active payloads in reverse activation order; addon cleanup failure never
widens cleanup to another process tree or installation.

## Renderer Addon Composition

Exactly one component owns an official instance, profile, `CODEX_HOME`, process
tree, CDP listener, and cleanup lease. Renderer addons never own those
resources. They are reviewed payloads composed after the Administrator
bootstrap inside the already selected host adapter.

The embedded schema-v2 allowlist is generic rather than skin-specific. Each
entry declares an ID and display name, exact project revision, supported host
adapters, stable load order, exclusive slots and explicit conflicts, one
reviewed JavaScript entrypoint, typed substitutions for any number of reviewed
UTF-8 or data-URL assets, and a namespaced lifecycle state/dispose method. All
files remain in a user-supplied external checkout and must match their SHA-256
and size bounds.

Planning occurs before execution. Settings are deduplicated and host-gated;
candidate payloads are ordered by `load_order` then ID. The first successfully
loaded payload owns each exclusive slot. Explicit or slot conflicts disable the
later payload and identify the blocker. Unknown IDs, invalid settings, missing
files, changed hashes, and incompatible hosts disable only the affected addon.

The project-owned runtime creates one namespaced registry per renderer
generation. Every addon installer runs behind an exception boundary and must
publish its reviewed lifecycle state before it becomes active. A failed
installer is recorded without blocking later payloads. Registry reload first
disposes the prior generation; normal disposal is reverse-order and idempotent.
If disposal fails, the prior registry retains the lifecycle handle, becomes
sealed against every new apply, and remains available for a bounded retry. The
bootstrap does not delete that registry until cleanup succeeds, so a failed
generation cannot silently mix with a replacement generation. The retained
record owns the original lifecycle object, not merely its global key, and
cleanup succeeds only when the namespaced global is absent; replacing that
global cannot masquerade as disposal.

The launch report state `enabled` means the reviewed assets were accepted and
prepared, not that arbitrary renderer code completed successfully. The native
manager calls the namespaced runtime health surface and matches both addon ID
and reviewed revision before describing an addon as active. It otherwise
reports pending, failed, unhealthy, or prepared state.
Health returns only sanitized addon IDs, revisions, status codes, and lifecycle
availability. It never includes checkout paths, source, arbitrary exception
text, or credentials.

Codex Dream Skin is integrated only through this payload route. Its upstream
Windows install/start/restore scripts and Node daemon are forbidden because
they can change global Codex configuration or process state. The current root
licensing does not clearly cover redistribution of Windows assets, so releases
read exact assets from the user's checkout and never embed them.

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

No Codex++ executable identity is currently reviewed because the shipped
compatibility host list is empty. Source familiarity, a local installation, or
a prior-version audit cannot enable it. Compatibility fallback does not launch
Codex++. A future adapter may be enabled only after an exact executable hash and
isolated profile/`CODEX_HOME`/SQLite, composition, and daily-PID-preserving E2E
prove that Codex++ is the sole owner of a disjoint instance. Codex++ must never
be launched inside the Direct Job Object.

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

Direct has two explicit root lifetimes. The default remains ephemeral and
deletes the whole owned root after process cleanup. The Windows supervisor opts into a
persistent isolated profile, a project ownership marker, and an exclusive root
lock; an unmarked existing directory or concurrent owner is rejected. The daily
profile is never used as the isolated `--user-data-dir`.

Provider configuration comes from the native selector manager's Base URL and
Windows Credential Manager entry. The non-secret settings cache only reviewed
`/v1/models` metadata and selected Grok IDs; loading or applying settings that
contain an unreviewed model fails closed. The provider credential is present
only in the launcher and child process environment, while inherited
`OPENAI_API_KEY` is removed before either official root process is created. The
generated isolated configuration forces the native shell environment filter on
and excludes the provider variable; provider configuration that reintroduces it
through `shell_environment_policy.set` fails closed.

The native selector manager enables native authentication synchronization by default. It reads the
daily `CODEX_HOME` only as a source and atomically copies validated `auth.json`;
the source is never modified. This is the file-backed API-key login state, not a
claim that keyring-only authentication can be cloned between independent homes.

Complete conversation import is separately gated and disabled by default. The
manager warns that it copies full history, may use several GB, and may send that
history to the selected Grok provider when continued. Its daily-state read
allowlist is exactly `sessions/**/*.jsonl`, `archived_sessions/**/*.jsonl`, and
`session_index.jsonl`. It never reads or copies daily `config.toml`, SQLite,
WAL, SHM, logs, goals, memories, junctions, symbolic links, or hard links.

Source rollouts use non-blocking shared reads. Before publication, the importer
checks the native file identity, size, modification time, complete line-valid
JSONL, full source hash, and a second stable read. Canonical provider metadata
is rewritten only in the private copy so the official thread scan exposes it
through `grok_native`. This proves visibility and routing only; reliable resume
and capability parity for arbitrary imported histories remain unproven.

Each daily `auth.json`, session index, and rollout source is opened with
`FILE_FLAG_OPEN_REPARSE_POINT` before handle-based shape, hard-link count,
identity, size, time, and hash validation. A final-component link or a file
replaced by a link after enumeration is rejected rather than followed.

The isolated SQLite home is pinned both in `config.toml` and
`CODEX_SQLITE_HOME` under the owned root. A per-file manifest verifies source
and destination hashes on every import, recovers a fully published update after
an interrupted manifest write, keeps one active-or-archived private copy per
thread, and gives any locally changed isolated rollout precedence. The private
`session_index.jsonl` uses official last-entry semantics and preserves a newer
isolated name. No imported state is written back to the daily `CODEX_HOME`.

Persistent shutdown still terminates the Job Object, tracks escaped
descendants to quiescence, and closes the loopback listener before releasing
the lock. The retained directory is intentional user state, not process
residue. Ephemeral runtime and E2E cleanup semantics remain unchanged.

The native selector manager and CLI do not log provider credentials, response bodies, command
arguments containing secrets, or model traffic. Model-fetch errors expose only
sanitized transport categories or HTTP status codes.

Control-broker settings updates are validated on a cloned candidate. A rejected
Base URL, model selection, sync option, or renderer-addon setting does not
mutate the in-memory state returned by later `state.read` calls. A pending
credential is written before the candidate settings are committed, is not
consumed on write failure, and is rolled back to the prior credential if the
atomic settings commit then fails.

Stored and pending credentials are bound to the Base URL plus effective Action
Path that they verified. Changing either endpoint component requires a fresh
key; a key discovered against one endpoint cannot be applied to another. The
Credential Manager blob stores the endpoint fingerprint with the secret rather
than trusting a mutable settings-file reference. A
control request that times out while queued is invalidated before it can be
drained. This renderer timeout does not cancel a request already drained into a
bounded broker operation; model discovery and settings commits retain their own
validation and operation timeouts.

Management-only startup has no selected reviewed model and passes no provider
credential into the official child environment. It retains only the native
selector management surface, removes stale Grok provider/catalog state from the
owned isolated home, skips provider readiness, and never routes a Grok task.
Both launcher stages scrub secret-shaped inherited variables, while configured
mode permits only the exact validated provider environment key.

The launcher snapshots every pre-existing ChatGPT PID before startup and
requires those PIDs to remain alive. It accepts only one
`app://-/index.html` target from its own loopback port. The CDP client validates
every command response, waits for bridge health and native UI readiness, and
reinstalls the same idempotent bootstrap when a renderer reload clears it. The
launcher then sends the official app-server `config/read` request through the
same renderer API and refuses readiness unless the returned configuration
contains `model_providers.grok_native`.

Each Chromium launch includes `--do-not-de-elevate`; without it, an
administrator process may relaunch outside the isolated environment and lose
the project-owned `CODEX_HOME` or credential environment variable. Missing
targets and renderer-health disconnects receive only a bounded recovery window;
package, PID, listener ownership, provider readiness, and target-identity
failures remain immediate fail-closed errors.

Each runtime-maintenance pass refreshes the full descendant lineage of both
launched roots and keeps process handles for descendants that escape Job
containment. Each tracked handle pins one PID generation with its creation and
exit times, keyed by `(PID, creation time)` so old and new generations can
coexist. The snapshot upper bound is recorded before snapshot creation. A
candidate is accepted only when its creation lies inside a trusted parent
generation and no later than that bound. Child entries that match only an old
generation remain pending while later parent entries may add a new generation.
This preserves orphan discovery but rejects unrelated PID reuse. A candidate
still present but inaccessible becomes permanent OwnedJob uncertainty. A
candidate that vanished before opening or whose PID was reused after the
snapshot becomes a temporary lineage anchor; the replacement is never tracked
or terminated. Shutdown still terminates every known handle, but reports
persistent uncertainty instead of claiming full cleanup. Descendant termination
uses a thirty-second cleanup budget after the initial global scan; exact-root
deletion retains a separate 10-second deadline. Errors never authorize broader
deletion or unrelated termination.

The launcher proves that it passed the isolated profile argument and the
dated official-host package E2E proves that the packaged host honored it. This
is compatibility evidence for the unreleased project build, not a Codex
Administrator release. The launcher does not fabricate a runtime profile
observation by copying the contract value back into its own verification input.

The provider route does not require or launch a separate Grok desktop client or
execution host.

## Capability Boundary

Seeing and selecting a model proves only model-list and routing behavior.
Streaming, tools, files, images, structured output, reasoning controls,
cancellation, and reliable resume are independent evidence gates. The project
must not advertise any of them from model visibility alone. Exact-model live
evidence for `grok-4.5` proves a valid public Responses stream, a native
app-server thread and text turn, and one `update_plan` function-call/output
round trip. A later isolated official-desktop run also proves one native shell
`commandExecution` with observed output and exit code `0`. It does not prove
files, images, parallel tools, structured output, cancellation, reliable
resume, or complete parity. A retained-profile run also proves that an
identical bootstrap reapplication preserves `grok_native` routing and that a
native turn stores `effort = high`. Parallel-tool behavior remains outside the
durable compatibility claim until an exact-model client and endpoint gate
passes; transient upstream status codes remain operational evidence only.

Direct descendant cleanup uses bounded repeated process snapshots rather than a
kernel process-creation trace. A process-open failure is rechecked against a
second system snapshot before it can become permanent uncertainty. Vanished or
post-snapshot reused PIDs are retained as temporary lineage anchors for the
five-second quiescence window. Their
replacement process is never terminated; any visible transitive descendant from
an active anchor refreshes that observation window and becomes a temporary
lineage member. This preserves taint when an intermediate member exits but a
grandchild survives. PPID edges from main snapshots and process-open rechecks
remain in a five-second lineage history, allowing later captures to reconnect
through an exited member. Historical edges establish topology only; a PID must
also be visible in the current main/recheck snapshot to refresh the window, and
expired anchors/members/edges are pruned before capture inputs are exported. A
current child with a known parent PID but ambiguous generation becomes a visible
temporary anchor/member and is not terminated from that ambiguous relation.
Cleanup fails closed if the chain persists, while a
chain that disappears still requires five continuous empty seconds. Clock
rollback within or across snapshots, parent-exit timestamp equality, and cleanup
completion after the strict deadline are rejected. Explicit shutdown and Drop
start the thirty-second descendant cleanup budget after the initial global scan.

PPID lineage is not the only ownership signal during shutdown. Official plugin
sync may be spawned by an external broker, so Direct also uses
`NtQueryInformationProcess(ProcessCommandLineInformation)` on current process
handles. It matches only an executable already inside the root or supported Git,
PowerShell, and Chromium path-argument grammar; arbitrary root text, `--`, and
command/message payloads do not match. Git `-C` options are accumulated to the
final cwd, drive roots stay absolute, and repeated Git path options use their
final values. Relative path options resolve only against a proved final `-C` cwd.
The queried image selects the parser; command-line `argv[0]` is
never image evidence. Query
uses only `PROCESS_QUERY_LIMITED_INFORMATION`; termination and synchronization
rights use a separate handle, and creation time must match before termination.
Query-only liveness uses `GetExitCodeProcess`. Supported broker query
failures without an exact match do not widen ownership; after an exact match,
termination-right or identity failure is fail-closed. The same scan and process wait share the
separate 10-second owned-root removal deadline.
