# Architecture

## V1 modes

`grok_injected_main` and `native_gpt_main` identify the native runtime that owns the main-agent loop. They are not model aliases.

The companion keeps a canonical task identity and maps it to runtime-owned sessions. Native transcripts remain owned by their respective clients. Shared state contains reviewed preferences, durable facts, decisions, checkpoints, and evolution records.

## Process boundary

The Windows launcher selects one host adapter. `direct` activates the official ChatGPT/Codex host with a loopback-only remote debugging endpoint and owns the sole CDP injection lifecycle. Optional `codexplusplus` launches an installed official Codex++ release and mounts the same project-owned bootstrap through its supported extension surface. The adapters are mutually exclusive for one host instance.

The companion binds a random loopback port and renders a per-launch namespaced bootstrap. It does not modify official application files or Codex++ binaries.

The bootstrap mounts a small mode control. Grok mode displays a companion-served, same-origin UI. Native mode disposes all layout, focus, keyboard, and visibility overrides and returns control to the official page.

## Runtime adapters

Each adapter owns its native process and session identifiers. The common broker contract covers probing, capability negotiation, starting or resuming a session, submitting a turn, streaming events, approval decisions, cancellation, checkpoints, and health.

Unknown versions and capabilities fail closed. A model name alone never enables a capability.

## Concurrency

One agent owns write access to a checkout at a time. Parallel agents use disjoint write sets or separate worktrees. The task graph records leases and terminal state so restart and cancellation cannot silently create two writers.

## Reference boundary

Codex++ is an architectural reference and optional external host adapter. Codex Administrator does not link, vendor, patch, or copy it. Only the `codexplusplus` adapter consumes its supported user-script and lifecycle surfaces; the direct adapter and shared core do not.

Implementation is derived independently from Windows, Electron, and CDP public interfaces, official runtime behavior, and project-owned tests. The direct adapter health-checks and reinjects through its own CDP session. The Codex++ adapter version-gates recovery and never adds a second injector.

## Update isolation

Host updates are independent and never patched or blocked. Each adapter must report an exact host identity that exists in the verified compatibility manifest before Grok injection is enabled. Unknown versions fail closed to native GPT mode; they do not prevent the official host from launching.
