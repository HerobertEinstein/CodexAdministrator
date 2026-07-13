# Codex Administrator

Codex Administrator is an open-source Windows launcher for using Grok models
through the official ChatGPT/Codex agent host without installing or trusting a
separate Grok client.

The official Codex host keeps ownership of the agent loop, tools, approvals,
sandbox, workspace, sessions, compaction, and user interface. Grok is only a
Responses-compatible model provider.

## Architecture

```text
Codex Administrator
  -> atomically registers [model_providers.grok_native]
  -> selects model_provider = "grok_native" and the requested Grok model
  -> preserves the previous native model/provider/catalog selection
  -> stores an environment-variable name, never the API key
  -> launches the official `codex app <workspace>`
       -> native Codex tools, approvals, sandbox, workspace, and sessions
```

The primary launch path does not patch ChatGPT/Codex, replace `app.asar`, inject
a Grok chat workspace, run Grok CLI/Grok Build, or add an independent Grok tool
and permission loop. Official application and Codex++ installers, binaries,
profiles, and updaters remain publisher-owned.

An optional Codex++ compatibility bridge remains fail-closed behind exact
executable SHA-256 evidence. Its iframe is hidden and cannot replace or cover
the official interface. The shipped alpha manifest contains no accepted host
identity, so unknown Codex++ builds remain completely native.

## Native Launch

Requirements:

- Windows 10 or later
- Rust 1.85 or later for source builds
- Official Codex CLI/Desktop installation
- A Grok endpoint that implements the Responses wire API at a `/v1` base URL
- A credential already available to the official ChatGPT/Codex process through
  the named environment variable

Provision the environment variable before starting ChatGPT/Codex. Fully exit
and reopen the official app after changing the Windows user environment so its
backend receives the new value. Then launch Grok:

```powershell
cargo run -- launch `
  --base-url "https://trusted-grok-endpoint.example/v1" `
  --env-key "XAI_API_KEY" `
  --model "grok-4" `
  --model-catalog "D:\Path\To\reviewed-grok-models.json" `
  --workspace "D:\VSC\work"
```

The launcher writes only the environment-variable name to Codex configuration.
It never accepts an `--api-key` argument and never prints or persists the
credential value. Remote endpoints require HTTPS; HTTP is accepted only for
`127.0.0.1`, `localhost`, or `::1` development endpoints.

Codex CLI 0.142.3 accepts root `-c` flags next to `codex app`, but its official
Windows `app` implementation does not forward those overrides to the desktop
process. The launcher therefore writes the supported user configuration
atomically and invokes only the real official desktop-open command:

```powershell
codex app "D:\VSC\work"
```

`--model-catalog` is optional for experimental fallback metadata, but required
before the project describes a Grok model as having reviewed model-list,
context, reasoning, tool, or modality metadata. The catalog is passed through
Codex's official persisted `model_catalog_json` surface, must use the current
official model-entry schema, and must contain the selected exact model slug.
Because that setting replaces the bundled catalog for the process, an existing
catalog is never silently replaced without an explicit `--model-catalog`.
Before launch, the installed official Codex runtime must also accept the file
through `codex debug models`.

Return to the exact model/provider/catalog selection that existed before Grok:

```powershell
cargo run -- launch-native --workspace "D:\VSC\work"
```

Restoration is fail-closed. If the user changed those fields after Grok was
selected, the launcher refuses to overwrite the newer choice.

## Capability Boundary

A visible model or successful text response is not proof of complete native
support. Responses streaming, tool calls, parallel tool calls, structured
outputs, image input, file input, reasoning summaries, cancellation, and error
behavior require capability-specific E2E evidence through the official Codex
host. Unknown capabilities default to disabled in the project capability
contract.

ChatGPT service-only tools and entitlements cannot be promised for a custom
provider. The project targets parity with the native local Codex agent
environment, and publishes a capability only after that exact model/endpoint
combination passes the relevant tests.

## Non-goals

- Modifying official ChatGPT/Codex or Codex++ installation files or updaters.
- Restoring Grok CLI, Grok Build, ACP, or a Grok-owned main-agent runtime.
- Showing a custom Grok chat UI over the official client.
- Persisting API keys in source, config, launch arguments, logs, or evidence.
- Claiming model, tool, streaming, or multimodal parity from model discovery.
- Bundling proprietary OpenAI, xAI, or Codex++ binaries.

## Development

Inspect the local environment without printing credentials:

```powershell
cargo run -- doctor --json
```

Run the verification suite:

```powershell
cargo fmt --check
cargo test --all-targets
node --test tests/ui_assets.test.mjs
cargo clippy --all-targets --all-features -- -D warnings
cargo test --test codex_live -- --ignored --nocapture --test-threads=1
```

See [ARCHITECTURE.md](docs/ARCHITECTURE.md),
[COMPATIBILITY.md](docs/COMPATIBILITY.md),
[UPDATE_ISOLATION.md](docs/UPDATE_ISOLATION.md), and
[RUNTIME_ADAPTERS.md](docs/RUNTIME_ADAPTERS.md) before changing provider,
launcher, host-compatibility, update, or removal behavior.

## Independence

Codex Administrator is an independent community project. It is not affiliated
with, endorsed by, or sponsored by OpenAI, xAI, X Corp., or Codex++. Product
names and trademarks belong to their respective owners.

## License

Apache-2.0. See [LICENSE](LICENSE).
