---
type: constraint
status: active
created: 2026-07-13
updated: 2026-07-14
scope: project
paths:
  - assets/renderer-api-discovery.js
  - assets/bootstrap.js
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
verified_by:
  - cargo test --test direct_instance_isolation
  - node --test tests/renderer_api_discovery.test.mjs tests/bootstrap_runtime.test.mjs
  - isolated official-desktop Playwright/CDP probe on OpenAI.Codex 26.707.8479.0
  - production Direct launcher Playwright/CDP E2E on OpenAI.Codex 26.707.8479.0
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
a disjoint isolated ChatGPT process tree, preserved all eight pre-existing
daily processes present during the run, routed a Grok `thread/start` probe
through `grok_native`, left the native bridge reference unchanged, restored the
exact prior renderer function, and removed every temporary process and profile
directory.

The production launcher now creates root processes suspended, verifies the
actual image and official package family, assigns them to a Windows Job Object
before resume, requires the CDP listener PID to belong to that Job, waits for
bridge health and UI readiness, and monitors reinjection with a bounded
target/health recovery window. Instance creation, writes, and removal reject
reparse-point ancestors. Fresh native UI E2E preserved all GPT entries,
appended Grok once, selected Grok, restored GPT-5.4, recovered after renderer
reload, preserved all eight daily PIDs, and cleaned every isolated process and
directory.

## Use Next Time

Keep every Direct launch behind the production process owner, two-stage
isolated window launch, suspended package verification, listener ownership,
CDP target monitor, UI readiness gate, bounded reinjection lifecycle, and exact
cleanup. Never fall back to attaching or activating the daily instance. Do not
confuse implementation or E2E evidence with a release, merge, deployment, or
endpoint capability-parity claim.
