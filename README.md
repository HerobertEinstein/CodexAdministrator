# Codex Administrator

Codex Administrator is an experimental Windows launcher component that adds
Grok entries to the native ChatGPT/Codex model list. It keeps the official
interface, GPT entries, agent loop, tools, approvals, sandbox, workspace, and
task storage under the original host's control.

## Exact Scope

The project has two narrowly separated responsibilities:

1. Register `[model_providers.grok_native]` in the supported user-owned Codex
   configuration without changing `model`, `model_provider`, or any native GPT
   entry.
2. Append configured Grok descriptors to a native `model/list` response and add
   `modelProvider: "grok_native"` only when a Grok model starts a new task or a
   previously identified Grok task is resumed.

It does not create another interface or replace the host's execution loop.
Selecting GPT preserves the original request object and behavior.

```text
Codex Administrator
  +-- configure-provider
  |     `-- writes only [model_providers.grok_native]
  `-- inject
        +-- appends Grok to model/list
        `-- routes Grok thread/start and known Grok thread/resume

Official ChatGPT/Codex host
  `-- keeps UI, GPT models, tools, approvals, sandbox, workspace, and tasks
```

## Current Status

This repository is alpha software and does not yet claim end-to-end Grok
support in the official desktop application.

- Provider registration is implemented and covered by configuration tests.
- The model-list and per-task routing bridge is covered by message-level tests.
- A real official-desktop probe has proved a separate profile, process tree,
  CDP port, frozen-bridge preservation, writable renderer-API composition,
  exact disposal, and daily-process survival. Direct injection remains
  disabled until those checks are enforced by the production launcher and
  target monitor.
- The Codex++ adapter writes only an external user script. The shipped
  compatibility manifest is empty, so unverified releases remain native and
  any stale project script is removed.
- Model visibility does not prove text streaming, tools, files, images,
  structured output, cancellation, resume reliability, or feature parity.

## Provider Configuration

Set the credential in an environment variable inherited by ChatGPT/Codex, then
register the provider:

```powershell
$env:XAI_API_KEY = "your-key"

cargo run -- configure-provider `
  --base-url "https://api.x.ai/v1" `
  --env-key "XAI_API_KEY"
```

The command accepts the environment-variable name only. It never accepts,
prints, or stores the key value. Remote URLs require HTTPS and must end in
`/v1`; loopback HTTP is accepted for tests.

## Model-List Injection

The direct adapter currently fails closed because the production isolated
launcher is not implemented:

```powershell
cargo run -- inject --host direct --model grok-4
```

For a separately installed and explicitly verified Codex++ release:

```powershell
cargo run -- inject `
  --host codexplusplus `
  --model grok-4 `
  --codex-plus-path "C:\Path\To\codex-plus-plus.exe"
```

An exact executable SHA-256 and matching E2E evidence must be present in
`compatibility.json`. Unknown releases are launched without this project's
script when `--no-launch` is omitted.

## Update Isolation

Codex Administrator never edits official installation directories, packaged
resources, executables, signatures, the daily profile, updater services,
update settings, or update channels. A future direct adapter may create only
its own isolated profile and isolated `CODEX_HOME`; neither path may equal,
contain, or be contained by a daily path. Project-owned writes are limited to:

- the exact `model_providers.grok_native` entry in user configuration;
- project-owned isolated profile and `CODEX_HOME` data for a direct instance;
- the exact Codex++ external user-script file and enablement key when that
  adapter passes its compatibility gate.

Official updates may require renewed compatibility evidence, but they remain
fully owned and installed by their publishers.

## Development

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
node --test tests/*.test.mjs
```

See [Architecture](docs/ARCHITECTURE.md),
[Compatibility](docs/COMPATIBILITY.md),
[Host adapters](docs/HOST_ADAPTERS.md), and
[Update isolation](docs/UPDATE_ISOLATION.md).
