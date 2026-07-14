# Update Isolation

Codex Administrator remains external to ChatGPT/Codex and Codex++ installation
and update mechanisms.

## Project-Owned Writes

The project may write only:

- `model_providers.grok_native` in the supported user-owned Codex
  configuration;
- a project-owned isolated profile and isolated `CODEX_HOME` for a direct
  instance;
- its generated Codex++ external script; and
- its exact Codex++ script enablement key.

Provider registration never changes the user's native model, default provider,
model catalog, existing providers, or unknown future settings.

## Forbidden Writes

The project must not edit or replace installation directories, executables,
packaged resources, signatures, native launchers, the daily profile, updater
services, update settings, or update channels. An isolated profile or
`CODEX_HOME` must not equal, contain, or be contained by any daily path. The
instance path must not traverse a reparse point. The project must not store
credential values in source, arguments, configuration, logs, reports, tests,
or compatibility evidence.

## Fail-Closed Updates

An unreviewed Codex++ update disables injection for that exact host identity.
A Direct update must still pass the protected package path, suspended image
and package-family checks, separate process tree, listener-PID ownership,
isolated target, bridge health, UI readiness, and official app-server provider
readiness gates. Failure terminates only the project Job Object and captured
descendant lineage, then removes only the owned instance root after a bounded
retry. It does not block publisher startup or change native GPT behavior. A
provider validation failure occurs before the isolated root is created; a
runtime provider-readiness failure cleans the root instead of claiming success.

## Removal

Removal may delete only project-owned entries, files, and isolated instance
directories after their absolute paths pass the isolation contract. It must
preserve all native models, user defaults, other provider definitions, daily
profiles, tasks, credentials, caches, and updater state.
