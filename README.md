# Codex Administrator

Codex Administrator is an open-source Windows launcher and local companion for running two explicit main-agent modes inside the official ChatGPT/Codex desktop host:

- **Grok injected main agent**: a dedicated injected workspace uses the official Grok Build CLI as the main agent. Official Codex CLI can run as a subagent or parallel peer.
- **Native GPT main agent**: the injected workspace is disposed and the official GPT/Codex interface and agent loop remain native.

This project is in early development. The current repository establishes the security, update, and mode-switch contracts before runtime integration.

## Architecture

```text
Official ChatGPT/Codex desktop
  -> Codex Administrator host adapter
       -> direct launcher and sole project-owned CDP session
       -> optional installed Codex++ compatibility path
       -> small namespaced dual-mode bootstrap
            -> native GPT UI
            -> companion-served Grok UI

Codex Administrator companion
  -> official Grok Build CLI
  -> official Codex CLI/app-server
  -> task/session graph
  -> preference/memory/evolution graph
  -> workspace/worktree arbitration
```

The injected script is a view bootstrap. It does not own credentials, CLI processes, approvals, memory, or filesystem execution.

## Non-goals

- Modifying official ChatGPT/Codex installation files or `app.asar`.
- Replacing the Codex agent loop by listing a Grok model in Codex.
- Running a second competing injector in the same ChatGPT/Codex instance.
- Copying, linking, or deriving Codex++ source code.
- Making Codex++ mandatory for the direct launcher or shared companion.
- Bundling or redistributing proprietary OpenAI, xAI, or Codex++ binaries.
- Claiming native capability parity without runtime-specific E2E evidence.

## Update boundary

ChatGPT/Codex, Codex++, Grok Build, and Codex CLI keep their own official update channels. Codex Administrator detects installed versions and gates compatibility instead of patching those installations. Codex++ is optional: users may select an installed official release as the host adapter, while the direct adapter remains independent.

## Development

Requirements:

- Windows 10 or later
- Rust 1.85 or later
- Official ChatGPT/Codex desktop for live injection tests
- Official Grok Build and Codex CLI for runtime integration tests

Inspect the local environment without printing credentials:

```powershell
cargo run -- doctor --json
```

Run the current companion and prepare the optional installed Codex++ adapter:

```powershell
cargo run -- serve `
  --host codexplusplus `
  --codex-plus-path "C:\Path\To\codex-plus-plus.exe"
```

The alpha currently implements the companion, secured injected UI, mode switching, Codex++ bootstrap preparation, compatibility policy, and native runtime launch contracts. Direct ChatGPT activation/CDP ownership and live Grok ACP/Codex app-server turns remain under active development and fail closed rather than claiming support.

Run the local checks:

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```

See [ARCHITECTURE.md](docs/ARCHITECTURE.md), [COMPATIBILITY.md](docs/COMPATIBILITY.md), [RUNTIME_ADAPTERS.md](docs/RUNTIME_ADAPTERS.md), and [SECURITY.md](SECURITY.md) before contributing to privileged launcher, injection, or runtime code.

## Independence

Codex Administrator is an independent community project. It is not affiliated with, endorsed by, or sponsored by OpenAI, xAI, X Corp., or the Codex++ project. Optional Codex++ interoperability uses an installed external release and contains no Codex++ source code. Product names and trademarks belong to their respective owners.

## License

Apache-2.0. See [LICENSE](LICENSE).
