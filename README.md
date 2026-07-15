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
   `modelProvider: "grok_native"` only after that response confirms the model
   was appended without a native-ID collision, when it starts a new task or a
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
- The model-list and per-task routing bridge is covered by message-level tests;
  routing starts empty until a native `model/list` response confirms each
  append, and a configured ID that collides with a native model remains native.
- The production Direct launcher now creates a unique profile and `CODEX_HOME`,
  verifies the suspended image and official package family, assigns each child
  to a Windows Job Object before resuming it, performs the required two-stage
  launch, proves the CDP listener PID belongs to that Job, waits for bridge and
  UI readiness, requires the official app-server `config/read` response to
  contain `model_providers.grok_native`, tolerates bounded renderer-reload
  transitions, monitors reinjection, rejects reparse-point ancestors, and
  removes only its instance root during shutdown. Chromium receives
  `--do-not-de-elevate` so its administrator relaunch retains the isolated
  environment and provider credential.
- Runtime maintenance continuously captures descendant process lineage and
  keeps handles plus process creation/exit times for children that escape Job
  containment. A child is owned only when its creation falls inside a tracked
  parent generation and no later than the process snapshot. Multiple
  generations of one PID remain distinct, and mismatched child entries retry
  after later parent entries. Inaccessible, vanished, or post-snapshot reused
  PIDs become permanent fail-closed uncertainty instead of widening ownership.
  Shutdown terminates the Job Object plus every tracked handle; any snapshot
  failure remains an error even if a later retry succeeds.
- A fresh official-desktop E2E on `OpenAI.Codex 26.707.9981.0` passed automatic
  executable discovery, bridge and native UI readiness, native
  `grok_native` provider readiness, daily-instance preservation, clean launcher
  exit, and zero remaining owned processes or instance-root residue.
- A subsequent exact-model run used `grok-4.5` through the same isolated native
  app-server. Its rollout recorded `HEBOX_NATIVE_GROK_OK`, one `update_plan`
  function call, the matching `function_call_output`, `HEBOX_TOOL_OK`, and two
  completed tasks. Natural session expiry preserved all eight daily PIDs and
  left zero launcher, listener, owned process, or instance-root residue.
- The final generation-safe cleanup run also exited naturally with empty
  stderr, preserved all eleven processes present in the daily ChatGPT tree at
  launch, and left zero owned process, listener, or instance-root residue.
- This implementation exists only on the feature branch. It is not released or
  deployed, and it has not been merged into the default branch.
- The Codex++ adapter writes only an external user script. The shipped
  compatibility manifest is empty, so unverified releases remain native and
  any stale project script is removed.
- Model visibility does not prove text streaming, tools, files, images,
  structured output, cancellation, resume reliability, or feature parity.
  A live exact-model E2E now proves public Responses streaming and native
  ChatGPT/Codex app-server text for `grok-4.5`, plus one `update_plan` function
  call and matching `function_call_output`. Files, images, shell tools, parallel
  tools, structured output, cancellation, resume reliability, and complete
  parity remain unclaimed. The separate `grok-4.5-cli` alias currently returns
  HTTP 503 because its upstream distributor has no available channel.

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

Direct instances receive the same provider entry in their isolated
`CODEX_HOME`; the daily configuration is not used or modified.

## Model-List Injection

Validate a Direct launch without creating a profile or starting a process:

```powershell
cargo run -- inject `
  --host direct `
  --model grok-4 `
  --base-url "https://api.x.ai/v1" `
  --env-key "XAI_API_KEY" `
  --no-launch
```

Remove `--no-launch` to start the owned official instance. The launcher remains
the foreground lifecycle owner so Ctrl+C, startup failure, target failure, or a
configured session timeout can terminate only its Job Object and remove only
its project-owned instance root.

```powershell
cargo run -- inject `
  --host direct `
  --model grok-4 `
  --base-url "https://api.x.ai/v1" `
  --env-key "XAI_API_KEY"
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
update settings, or update channels. A Direct instance may create only its own
isolated profile and isolated `CODEX_HOME`; neither path may equal, contain, or
be contained by a daily path. Project-owned writes are limited to:

- the exact `model_providers.grok_native` entry in user configuration;
- project-owned isolated profile and `CODEX_HOME` data for a direct instance;
- the exact Codex++ external user-script file and enablement key when that
  adapter passes its compatibility gate.

Official updates may require renewed compatibility evidence, but they remain
fully owned and installed by their publishers.

The provider is an API route inside the official host. The project does not
bundle, launch, or depend on a separate Grok desktop client, interface, CLI, or
agent runtime.

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
