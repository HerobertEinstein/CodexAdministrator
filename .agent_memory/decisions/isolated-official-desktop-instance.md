---
type: constraint
status: active
created: 2026-07-13
updated: 2026-07-17
scope: project
paths:
  - assets/renderer-api-discovery.js
  - assets/bootstrap.js
  - assets/provider-readiness.js
  - src/isolation.rs
  - src/direct.rs
  - src/cdp.rs
  - src/windows_runtime.rs
  - src/main.rs
  - tests/direct_instance_isolation.rs
  - tests/direct_launcher_contract.rs
  - tests/loopback_cdp_client.rs
  - tests/windows_direct_runtime.rs
  - tests/bootstrap_runtime.test.mjs
  - tests/provider_readiness.test.mjs
verified_by:
  - cargo test --test direct_instance_isolation
  - node --test tests/renderer_api_discovery.test.mjs tests/bootstrap_runtime.test.mjs
  - node --test tests/provider_readiness.test.mjs
  - 2026-07-17 production Direct launcher E2E on OpenAI.Codex 26.707.12708.0
---
# Isolated Official Desktop Instance

## Summary

Direct injection may target only a separately launched official desktop
instance. The daily profile, daily `CODEX_HOME`, pre-existing process tree, and
daily CDP surface are forbidden. Failure to prove a separate profile, process
tree, loopback port, and `app://-/index.html` target fails closed.

The retained local entry synchronizes validated daily `CODEX_HOME/auth.json` by
default. Complete conversation import is a separate explicit opt-in and is
limited to `sessions/**/*.jsonl`, `archived_sessions/**/*.jsonl`, and
`session_index.jsonl`. It never copies daily `config.toml`, SQLite/WAL/SHM,
logs, goals, or memories. Both flows are one-way into private state; daily
sources remain read-only. Hard-linked auth or rollout inputs are rejected.

When no reviewed model is selected, the supervisor starts in management-only
mode. It may report saved-key presence to the native selector manager, but it
does not pass the provider credential to the official child, removes stale
Grok provider/catalog state from the retained isolated home, skips provider
readiness, and enables no Grok routing. Common secret-shaped inherited
variables and the explicit management provider env key are removed before the
official process environment is built.

The official `electronBridge` is frozen, sealed, non-extensible, and its
`sendMessageFromView` property is non-writable. The safe composition point is
the writable renderer `postMessage` object exported by the same-origin
`vscode-api-*` module referenced by the official entry bundle.

## Evidence

The official package probe used a temporary profile and fresh CDP port, created
a disjoint isolated ChatGPT process tree, routed a Grok `thread/start` probe
through `grok_native`, left the native bridge reference unchanged, restored the
exact prior renderer function, and removed every temporary process and profile
directory. A 2026-07-17 update-compatibility run on
`OpenAI.Codex 26.707.12708.0`
passed automatic executable discovery, bridge and native UI readiness, official
app-server `grok_native` readiness, daily-instance preservation, clean launcher
exit, and zero owned process or instance-root residue.

The production launcher now creates root processes suspended, verifies the
actual image and official package family, assigns them to a Windows Job Object
before resume, passes `--do-not-de-elevate`, requires the CDP listener PID to
belong to that Job, waits for bridge and UI readiness, and refuses ready unless
the official app-server `config/read` response contains `grok_native`. It
monitors reinjection with a bounded target/health recovery window. Instance
creation, writes, and removal reject reparse-point ancestors. Shutdown captures
descendant lineage beyond Job containment during runtime maintenance, keeps
escaped-child handles under `(PID, creation time)` identities after an
intermediate parent exits, retries child entries after later parent generations,
and rejects reused or post-snapshot PIDs. A process-open failure triggers a
second system snapshot; only a PID still present but inaccessible becomes
permanent OwnedJob identity uncertainty.
A candidate already gone before opening, or a replacement PID created at or
after the snapshot boundary, becomes a temporary lineage anchor without opening
or terminating the replacement. Every visible process chain rooted at an active
anchor refreshes its observation window. Visible descendants are promoted to
temporary lineage members, so a surviving grandchild remains tainted after its
intermediate parent exits. Main-snapshot and process-open-recheck PPID edges are
retained for five seconds, allowing a later snapshot to reconnect that surviving
grandchild through the exited member. Historical edges provide topology only;
expired state is pruned before capture inputs are exported, and only a PID visible
in the current main/recheck snapshot refreshes the window. A
fixed-point child with a known parent PID but ambiguous generation becomes a
visible temporary anchor/member and is not terminated from that ambiguous edge.
A persistent chain therefore times out
fail-closed, while a chain that disappears must still be followed by five
continuous empty seconds. Known handles are still terminated, the first true
identity uncertainty or snapshot failure remains the final result, and shutdown
requires five continuous seconds of empty descendant captures before exact-root
removal. Backward time within or across snapshots, ambiguous parent-exit
equality, and strict deadline overruns fail closed. Explicit shutdown and Drop
use a thirty-second descendant cleanup budget that begins after the initial
global scan, leaving bounded room for the five-second quiescence window under
parallel load. Exact-root removal retains a separate 10-second deadline. This
is bounded repeated-snapshot monitoring, not a kernel process-creation trace.

Official plugin sync can also arrive through an external broker and therefore
have no owned PPID ancestry. Shutdown and every exact-root removal attempt query
process command lines with native `NtQueryInformationProcess`. A process matches
only when its executable is inside the root or supported Git, PowerShell, or
Chromium path-argument grammar names the exact root. Arbitrary executables, `--`,
and command/message payloads do not match. Multiple Git `-C` arguments resolve to
the final cumulative cwd, drive roots remain absolute, and repeated Git path
options use their final values. Relative path options resolve only against a
proved final `-C` cwd; an unknown initial cwd does not widen ownership. The queried image selects the parser;
command-line `argv[0]` never replaces image evidence. The scanner first uses a
non-synchronizing query-only handle, opens
termination/synchronization rights only after a match, and requires equal creation
times. Unreadable processes without an exact match do not widen ownership;
termination-right or identity failure after a match is fail-closed. Query-only
liveness uses `GetExitCodeProcess`, not running-process exit timestamps. Root scan,
termination wait, and deletion share one separate 10-second owned-root removal deadline. This
still covers isolated-root `git fetch` / `index-pack` helpers without
generalizing to unrelated Git processes.

Retained exact-model E2E evidence for `grok-4.5` covers native text, one
`update_plan` function-call/output loop, and one PowerShell
`commandExecution` with exit code `0`. Each dated run preserved the daily
process identities and left no launcher, listener, owned process, instance-root,
or stderr residue. Raw run markers and transient process counts stay in local
verification evidence rather than this active public decision.

A later retained-profile run produced equal SHA-256 hashes and sizes for the
daily and isolated `auth.json`, showed the native composer without a sign-in
prompt, kept the provider-specific credential separate from official login
state, and recorded a new high-effort Grok turn as
`modelProvider = grok_native` after an identical bootstrap reapplication.

The dated 2026-07-17 configured-provider run used official package
`OpenAI.Codex 26.707.12708.0`, native-login sync, and one exact isolated test
root. It preserved the pre-existing daily process identities, produced a clean
launcher exit, closed the owned endpoint across a stable failure window, and
removed its exact test root. Credential scans found no provider-key value in
source or retained evidence.

## Use Next Time

Keep every Direct launch behind the production process owner, two-stage
isolated window launch, suspended package verification, listener ownership,
CDP target monitor, UI and native provider readiness gates, bounded reinjection
lifecycle, descendant-lineage cleanup, and exact owned-root removal. Never fall
back to attaching or activating the daily instance. Do not confuse
implementation or E2E evidence with a release, merge, deployment, or endpoint
capability-parity claim. `grok-4.5` has exact live evidence for Responses text
plus one native `update_plan` loop and one native shell `commandExecution` loop.
Files, images, parallel tools, structured output, cancellation, resume
reliability, and complete parity remain unproven. Keep complete session import
off by default and consult `native-state-import-boundary.md` before widening its
daily read allowlist or making any resume claim. Transient upstream availability
belongs in local operational evidence, not this active architecture decision.
