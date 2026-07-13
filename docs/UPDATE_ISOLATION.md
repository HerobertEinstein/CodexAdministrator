# Update Isolation

Codex Administrator is external to official ChatGPT/Codex and Codex++
installations. An upstream update may change provider behavior, but the project
must not modify, replace, delay, pin, spoof, or roll back that update.

## What The Project Guarantees

- Official installation directories, executables, packages, signatures,
  profiles, updater services, update settings, and update channels remain
  publisher-owned and are never patched by this project.
- The official host does not depend on Grok provider configuration or a Codex
  Administrator component in order to start and use existing providers.
- Grok is configured only as a Responses-compatible model provider through the
  supported, user-owned host configuration surface.
- Provider configuration contains an environment-variable reference only;
  credential values are never persisted by the project.
- Invalid or incompatible provider configuration fails closed without changing
  official binaries, updater state, or unrelated user configuration.
- Explicit Grok launch may change only `model`, `model_provider`, and optionally
  `model_catalog_json`; their previous values are backed up and restored by
  `launch-native` only when no later user change is detected.
- Codex Administrator does not install Grok CLI, Grok Build, ACP, a custom
  main-agent UI, or a second tool/approval runtime into ChatGPT/Codex or
  Codex++.

## Configuration Boundary

The project may own its launcher, logs, compatibility evidence, the exact
`model_providers.grok_native` configuration entry, and the credential-free
native-selection backup. Explicit launch may also own the active `model`,
`model_provider`, and `model_catalog_json` values until restoration. Writes
must be atomic, idempotent, and preserve unknown or future fields.

The project must not:

- write into an official installation directory;
- replace an official executable, package, launcher, shortcut target, or
  updater;
- disable, defer, intercept, or spoof an official update;
- edit Codex++ binaries, bundled scripts, updater state, or installation data;
- store API keys, bearer tokens, or other credential values in configuration,
  logs, evidence, or source control;
- change the user's default model merely to register the provider rather than
  from an explicit model launch; or
- claim that a visible model has capabilities that have not been validated.

## Fail-Closed Update Behavior

After a host, provider, gateway, or model update, previously accepted capability
evidence is not automatically transferable. Any changed or unknown behavior
remains unsupported until it is revalidated through the official host path.

Failure disables only the affected provider or capability claim. It must not
block official startup, weaken approvals or sandboxing, patch the host, or
activate the superseded Grok CLI/ACP route.

## Removal Boundary

Disabling or uninstalling Codex Administrator may remove only project-owned
files and the exact project-owned provider entry. It must preserve all other
provider definitions, model defaults, host profiles, credentials,
conversations, caches, official files, Codex++ data, and updater state.

Any future removal implementation requires tests proving that preservation
boundary before it can be described as supported.

## Verification Requirement

Changes to provider configuration, compatibility validation, update behavior,
or removal logic require tests for fail-closed behavior, secret non-persistence,
unrelated-field preservation, and uninterrupted official-host operation. See
[COMPATIBILITY.md](COMPATIBILITY.md) for capability evidence requirements.
