---
type: constraint
status: active
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - assets/renderer-api-discovery.js
  - assets/bootstrap.js
  - src/isolation.rs
  - tests/direct_instance_isolation.rs
  - tests/bootstrap_runtime.test.mjs
verified_by:
  - cargo test --test direct_instance_isolation
  - node --test tests/renderer_api_discovery.test.mjs tests/bootstrap_runtime.test.mjs
  - isolated official-desktop Playwright/CDP probe on OpenAI.Codex 26.707.8479.0
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

## Use Next Time

Do not enable the direct adapter from message-level or manual probe evidence
alone. First implement the production process owner, two-stage isolated window
launch, CDP target monitor, reinjection lifecycle, and exact cleanup. Never
fall back to attaching or activating the daily instance.
