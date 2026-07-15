---
type: constraint
status: active
created: 2026-07-13
updated: 2026-07-15
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
  - production Direct launcher E2E on OpenAI.Codex 26.707.9981.0
---
# Isolated Official Desktop Instance

## Summary

Direct injection may target only a separately launched official desktop
instance. The daily profile, daily `CODEX_HOME`, pre-existing process tree, and
daily CDP surface are forbidden. Failure to prove a separate profile, process
tree, loopback port, and `app://-/index.html` target fails closed.

The official `electronBridge` is frozen, sealed, non-extensible, and its
`sendMessageFromView` property is non-writable. The safe composition point is
the writable renderer `postMessage` object exported by the same-origin
`vscode-api-*` module referenced by the official entry bundle.

## Evidence

The official package probe used a temporary profile and fresh CDP port, created
a disjoint isolated ChatGPT process tree, routed a Grok `thread/start` probe
through `grok_native`, left the native bridge reference unchanged, restored the
exact prior renderer function, and removed every temporary process and profile
directory. A later update-compatibility run on `OpenAI.Codex 26.707.9981.0`
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
use a ten-second absolute deadline that begins before the initial global scan,
leaving bounded room for the five-second quiescence window. This is bounded
repeated-snapshot monitoring, not a kernel process-creation trace.

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
termination wait, and deletion share one ten-second absolute deadline. This
still covers isolated-root `git fetch` / `index-pack` helpers without
generalizing to unrelated Git processes.

The `grok-4.5` native app-server run recorded exact text, one `update_plan`
function call and output, final text, and two completed tasks. Natural session
expiry preserved all eight daily PIDs and left no launcher, listener, owned
process, or instance-root residue.

The final generation-safe natural-timeout run preserved all eleven daily
ChatGPT processes present at launch and again left no launcher, listener, owned
process, instance root, or stderr residue.

The later r7 native shell run used a non-ephemeral thread only inside the
isolated `CODEX_HOME`. Its stored turn contained one completed PowerShell
`commandExecution`, output `HEBOX_DESKTOP_SHELL_TOOL_OK`, exit code `0`, and
final text `HEBOX_DESKTOP_SHELL_FINAL_OK`. Natural timeout preserved all eleven
daily PIDs and left no launcher, listener, owned process, instance root, or
stderr residue.

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
reliability, and complete parity remain unproven. The separate `grok-4.5-cli`
alias currently returns upstream HTTP 503.
