# Architecture

## Canonical Topology

The official ChatGPT/Codex host is the only main-agent host. It owns the native
agent loop, tool execution, approvals, sandbox policy, workspace access,
session history, compaction, cancellation, and user interaction.

Codex Administrator may register a Grok endpoint as a Responses-compatible
model provider through the official host's supported user configuration. Grok
supplies model inference only. It does not become a second agent runtime and
does not receive an independent tool, approval, sandbox, workspace, or session
implementation from this project.

The native launch path atomically writes the supported user-owned
`model_provider` and `model` fields, then invokes the official `codex app`
workspace-open command without ineffective `-c` arguments. Provider
registration alone does not change the default model; an explicit `launch`
does select Grok for the official host.

Before that selection, the launcher records the previous model, provider, and
catalog path in a project-owned sidecar outside official installation
directories. `launch-native` restores those exact values only while the active
configuration still matches the project-managed Grok selection. Manual user
changes cause restoration to fail closed.

Reviewed Grok model metadata is supplied through the official
`model_catalog_json` configuration surface. The launcher validates that the
catalog is a bounded JSON file using the required official model-entry shape
and containing the selected exact model slug before persisting its absolute
path. The installed official Codex runtime then parses the same file through
`debug models`; rejection stops launch. A missing catalog leaves Codex fallback
metadata in effect and cannot be described as full model parity.

## Modes

`grok_native_model` and `native_gpt_main` are model-selection intents within
the same official host. They are not runtime, process, protocol, or capability
aliases. Switching models must not replace or bypass host-owned controls.

The host remains usable when the Grok provider is absent, invalid,
unauthenticated, incompatible, or disabled. Failure to select Grok never
blocks native host startup.

## Provider Boundary

The Grok provider contract is limited to the Responses wire API at a validated
`/v1` endpoint. Remote endpoints require HTTPS; plain HTTP is accepted only for
an explicit loopback endpoint. Provider configuration stores the name of an
environment variable, never the credential value.

The project may update only the supported, user-owned provider and explicit
selection fields. It preserves unrelated settings. Registration alone does
not change the user's selection; `launch` changes it deliberately and keeps a
restorable backup.

## Capability Validation

Model discovery proves only that the host or endpoint exposes a model
identifier. It does not prove parity with native GPT models or any other model.
Streaming, tool calls, structured outputs, image input, reasoning metadata,
resume behavior, cancellation, and every other capability require separate
validation through the official host path.

Unknown, missing, or contradictory capability evidence fails closed. The
project must not synthesize parity through a Grok CLI, ACP bridge, custom tool
runner, relaxed approval policy, broader sandbox, or hidden fallback. The
provider remains unavailable for the unsupported operation, while the official
host and its existing providers continue unchanged.

`NativeProviderCapabilityManifest` records exact model identifiers, explicit
capability booleans, and an immutable evidence SHA-256 digest. Omitted fields
default to `false`; a manifest for one model never grants another model the
same capabilities.

## Ownership And Updates

Official ChatGPT/Codex and Codex++ installations, binaries, packages,
signatures, profiles, updater services, update settings, and update channels
remain publisher-owned. Codex Administrator does not patch, vendor, replace,
pin, delay, or roll back them. It also does not install a Grok main-agent route
into either product.

Project-owned configuration and evidence must remain outside official
installation directories. Removing or disabling the provider may remove only
the exact project-owned provider entry; it must preserve all other user and
host data.

## Concurrency

The official host remains the authority for workspace and session ownership.
At the project level, one agent owns write access to a checkout at a time.
Parallel agents use disjoint write sets or separate worktrees.

## Superseded Route

An earlier design explored a companion-served Grok main-agent UI backed by the
official Grok CLI and ACP. That route was abandoned for security reasons. It is
historical context only and is not an active adapter, fallback, compatibility
target, or support claim.
