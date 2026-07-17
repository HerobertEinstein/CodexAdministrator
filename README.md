# Codex Administrator

Codex Administrator is an experimental Windows headless supervisor and
renderer extension that adds Grok entries to the native ChatGPT/Codex model
list. It keeps the official interface, GPT entries, agent loop, tools,
approvals, sandbox, workspace, and task storage under the original host's
control.

## Exact Scope

The project has three narrowly separated responsibilities:

1. Register `[model_providers.grok_native]` in the supported user-owned Codex
   configuration without changing `model`, `model_provider`, or any native GPT
   entry.
2. Append configured Grok descriptors to a native `model/list` response and add
   `modelProvider: "grok_native"` only after that response confirms the model
   was appended without a native-ID collision, when it starts a new task or a
   previously identified Grok task is resumed.
3. Add a management entry to the official model selector for provider setup,
   bounded `/v1/models` discovery, Grok selection, optional one-way native-state
   import, and reviewed renderer-addon settings.

It does not create a separate application interface or replace the host's
execution loop. Selecting GPT preserves the original request object and
behavior.

```text
Codex Administrator
  +-- configure-provider
  |     `-- writes only [model_providers.grok_native]
  +-- headless supervisor
  |     `-- owns one isolated official instance
  `-- renderer extension
        +-- appends Grok to model/list
        +-- routes Grok thread/start and known Grok thread/resume
        `-- adds management inside the official model selector

Official ChatGPT/Codex host
  `-- keeps UI, GPT models, tools, approvals, sandbox, workspace, and tasks
```

## Current Status

This repository is alpha software. The implementation exists only on the
feature branch; it is not merged or publicly released, and it is not installed
or deployed. A local project-owned build can be produced without changing the
official package or daily ChatGPT instance.

- Provider registration, model-list injection, native-selector management, and
  per-task Grok routing are implemented with fail-closed collision handling.
- The Direct headless supervisor owns a separate official profile,
  `CODEX_HOME`, process tree, Windows Job Object, and loopback CDP port. It
  validates the official package before resume and never falls back to the
  daily instance.
- The most recent retained compatibility run on 2026-07-17 used
  `OpenAI.Codex 26.715.2305.0` in management-only mode with no selected Grok
  model or provider credential. It proved startup on that official package,
  the native model-selector manager, daily-instance preservation, and exact
  owned-root and loopback-port cleanup. Configured-provider execution on that
  exact package is not claimed by this run.
- Exact `grok-4.5` evidence covers Responses text streaming, native text, one
  `update_plan` function-call/output loop, one native shell
  `commandExecution` loop, and high reasoning selection. Files, images,
  parallel tools, structured output, cancellation, automated restart/resume,
  and complete native parity remain unclaimed.
- Native file-backed authentication can be copied one-way into the retained
  isolated profile. Full task snapshot import is a separate opt-in and remains
  a visibility/routing feature rather than a resume-parity claim.
- Reviewed renderer addons are currently enabled only for Direct. The shipped
  catalog reviews one exact Codex Dream Skin revision from a user-owned
  checkout; changed assets disable only that payload.
- Codex++ compatibility remains disabled. The shipped compatibility manifest is
  empty, so an unverified Codex++ build stays native and is not launched by
  Administrator.

## Windows Supervisor

Build both project-owned binaries, then start the headless supervisor. It opens
the isolated official ChatGPT/Codex interface:

```powershell
cargo build --release --locked
target\release\codex-administrator-launcher.exe
```

Open the official model selector and choose **Manage Grok models**. Enter an
OpenAI-compatible Base URL ending in `/v1` and an API key, refresh the bounded
live catalog, select one or more reviewed Grok IDs, then apply the settings.
The supervisor restarts only its isolated official instance.

The API key is stored as a Generic Credential in Windows Credential Manager.
It is never written to launcher settings, Codex configuration, model catalogs,
diagnostics, command arguments, stdout, or repository artifacts. The child
injector receives it only through the project-specific
`CODEX_ADMINISTRATOR_PROVIDER_API_KEY` process environment. Any inherited
`OPENAI_API_KEY` is removed before the official isolated process starts. The
isolated `shell_environment_policy` also excludes the provider variable, so
native shell tools cannot read it while provider requests continue to work.
Changing the Base URL or Action Path requires a fresh API key, verified against
that endpoint before it can replace the saved endpoint and credential.
The stored credential includes an endpoint fingerprint; replacing only
`launcher-settings.json` cannot redirect the saved key after restart and falls
back to management-only mode.

Management-only startup never passes the provider key to the official child.
With no reviewed model selected, the isolated official UI can report whether a
saved key exists and open the manager, but stale Grok provider/catalog state is
removed and no Grok task routing is enabled. The launcher also removes
secret-shaped inherited environment variables; a configured run restores only
its single project-specific provider variable.

The non-secret launcher settings live at
`%LOCALAPPDATA%\CodexAdministrator\launcher-settings.json`. They contain only
the Base URL, cached model metadata, selected IDs, native-state sync
preferences, and enabled renderer-addon IDs plus external checkout paths. They
never contain addon source, provider credentials, or copied third-party assets.
The last applied Grok selection remains launchable when the provider endpoint
is temporarily unavailable; refreshing the wider catalog still requires the
endpoint.

Remote URLs require HTTPS. Loopback HTTP is accepted only for local testing.
The editable first-run default is `https://ai.hebox.net/v1`, the project's
public operator endpoint; it includes no credential and can be replaced before
model discovery.
`/v1/models` is bounded, parsed without echoing response bodies, deduplicated,
and cached. The current unreleased reviewed capability registry admits only the
case-sensitive IDs `grok-4.5`, `grok-4.3-{low,medium,high}`, and
`grok-4.20-multi-agent-{low,medium,high,xhigh}`. Unreviewed Grok IDs are not
injected; unrelated models returned by the same endpoint are not mislabeled as
xAI models or given unverified capabilities. Registry admission does not prove
that a configured endpoint currently serves the model or has passed complete
native capability parity.

## Provider Configuration

The CLI remains available for diagnostics and automation. Set a provider key in
a process-local environment variable, then register the provider:

```powershell
$env:XAI_API_KEY = "your-key"

cargo run -- configure-provider `
  --base-url "https://api.x.ai/v1" `
  --env-key "XAI_API_KEY"
```

The command accepts the environment-variable name only. It never accepts,
prints, or stores the key value.

`OPENAI_API_KEY` is reserved and rejected because the official host may copy
that authentication variable into its isolated `CODEX_HOME/auth.json`. Use a
provider-specific name such as `XAI_API_KEY` instead.

Direct instances receive the same provider entry in their isolated
`CODEX_HOME`; the daily configuration is not used or modified.

## Model-List Injection

The exact `grok-4.5` descriptor follows the current xAI documentation: it
offers low, medium, and high reasoning effort, and the default is high.
Reasoning cannot be disabled for this model. Sources:
[Grok 4.5](https://docs.x.ai/developers/grok-4-5) and
[Reasoning](https://docs.x.ai/developers/model-capabilities/text/reasoning).
The current native UI labels the low preset as `Light`; stored turns retain the
wire value `low`, while `Medium` and `High` map directly.
The generated native catalog uses a 32,768-token conservative client cap for
every reviewed Grok ID. This is a fail-safe Codex compaction boundary, not the
provider's official maximum, and it is never copied from a GPT descriptor.
Reasoning summaries, search-tool support, and parallel tool calls remain
disabled unless exact model-specific evidence is added.

Validate a Direct launch without creating a profile or starting a process:

```powershell
cargo run -- inject `
  --host direct `
  --model grok-4.5 `
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
  --model grok-4.5 `
  --base-url "https://api.x.ai/v1" `
  --env-key "XAI_API_KEY"
```

### Persistent native state

The headless supervisor and native selector manager use a persistent isolated profile under
`%LOCALAPPDATA%\CodexAdministrator\instances\default`. An exclusive root lock
rejects a concurrent owner. The official daily profile, official installation,
updater, and process tree are never attached, restarted, closed, or modified.

Native authentication synchronization is enabled by default in the manager. It
validates and atomically copies only the daily `auth.json`. A source containing
the provider credential, including a credential embedded inside another JSON
string, is rejected. This covers the current file-backed API-key login; it does
not claim to mirror OS-keyring-only authentication modes.

Complete conversation import is a separate, one-way private import and is
disabled by default. Enabling it shows a confirmation that the full prompts,
messages, tool output, and environment history may require several GB and may be
sent to the selected Grok provider when a conversation is continued. The only
daily conversation inputs are `sessions/**/*.jsonl`,
`archived_sessions/**/*.jsonl`, and `session_index.jsonl` names. Daily
`config.toml`, SQLite/WAL/SHM files, logs, goals, memories, hard links,
junctions, and symbolic links are never copied or shared.

Rollouts are read without blocking the daily instance. The importer verifies
the file identity, size, modification time, complete JSONL shape, full hash, and
a second consistency pass before atomically publishing a private copy. The
private canonical metadata uses `grok_native` so imported threads are visible
and routed through the injected provider. Visibility and routing do not prove
reliable resume or capability parity for arbitrary historical content.

The isolated SQLite home is explicitly pinned inside the retained owned root.
A per-file manifest records source and destination hashes, recovers a fully
published update after an interrupted manifest write, and keeps only one private
active-or-archived copy for each thread. A locally changed private rollout wins;
later launches never overwrite it or write anything back to daily state. Active,
partial, malformed, reparse-backed, duplicate-ID, or conflicting files fail
closed and remain on the last known good private snapshot. Session names merge
by official append-only last-entry semantics, while a newer isolated name wins
without being written back to the daily `session_index.jsonl`.

On shutdown, all owned processes, escaped descendants, and the loopback
listener are still cleaned. The retained directory is intentional user state,
not process residue. Temporary Direct CLI launches continue to remove their
whole instance root by default.

No Codex++ executable is currently eligible: the shipped `compatibility.json`
host list is empty. The adapter code is a fail-closed future integration
surface, not an available runtime option. Unknown releases remove only a stale
Administrator-owned script/config key, report `native_fallback`, and are not
launched by Administrator. Enabling a future identity requires its exact
executable SHA-256 and matching host/composition E2E in the same publication.

## Compatible Renderer Addons

Renderer addons are optional payloads, not process owners. Direct mode owns the
isolated official instance. A future verified Codex++ mode would leave Codex++
as the sole instance owner and install one Administrator-owned composite script
through Codex++'s documented `user_scripts` extension slot. Each skin or UI
project then runs inside that one composite script.

`renderer-addons.json` is an allowlist of exact external assets. It declares the
reviewed project revision, supported host adapters, stable load order,
exclusive slots and explicit conflicts, an entrypoint plus typed asset
substitutions, and the namespaced lifecycle state/dispose method. The native
manager renders this catalog dynamically, so adding another reviewed project
does not require a project-specific UI branch.

The current entry pins commit
`568469a4f97e8fa4c8d237ce018c206c29959ecd` from
[Codex Dream Skin](https://github.com/Fei-Away/Codex-Dream-Skin); its listed
asset hashes must match that external checkout exactly.

The upstream checkout stays user-owned and read-only. Administrator never runs
an addon's install/start/restore scripts, daemon, shortcuts, process cleanup, or
global configuration writes, and it does not copy or redistribute external
assets. A missing, changed, conflicting, incompatible, or failing addon is
disabled independently while the validated host and Administrator bridge stay
available.

## Update Isolation

Codex Administrator never edits official installation directories, packaged
resources, executables, signatures, the daily profile, updater services,
update settings, or update channels. A Direct instance may create only its own
isolated profile and isolated `CODEX_HOME`; neither path may equal, contain, or
be contained by a daily path. Project-owned writes are limited to:

- the exact `model_providers.grok_native` entry in user configuration;
- project-owned isolated profile and `CODEX_HOME` data for a direct instance;
- the exact Codex++ external user-script file and enablement key when that
  adapter passes its compatibility gate; and
- non-secret renderer-addon settings plus read-only access to exact allowlisted
  files in user-supplied external checkouts.

Official updates may require renewed compatibility evidence, but they remain
fully owned and installed by their publishers.

The provider is an API route inside the official host. The project does not
bundle, launch, or depend on a separate Grok desktop client, interface, CLI, or
agent runtime.

## Development

```powershell
cargo fmt --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --locked
node --test tests/*.test.mjs
```

See [Architecture](docs/ARCHITECTURE.md),
[Compatibility](docs/COMPATIBILITY.md),
[Host adapters](docs/HOST_ADAPTERS.md), and
[Update isolation](docs/UPDATE_ISOLATION.md).
