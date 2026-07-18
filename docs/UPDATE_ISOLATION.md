# Update Isolation

Codex Administrator remains external to ChatGPT/Codex and Codex++ installation
and update mechanisms.

## Project-Owned State And Reads

The project may write only:

- `model_providers.grok_native` in the supported user-owned Codex
  configuration;
- `%LOCALAPPDATA%\CodexAdministrator\launcher-settings.json`, containing only
  non-secret provider, model, state-import, and renderer-addon settings;
- one per-user Generic Credential in Windows Credential Manager for the
  provider API key;
- a project-owned isolated profile and isolated `CODEX_HOME`, including its
  generated model catalog and SQLite location;
- an atomically validated private copy of daily `auth.json` when native login
  synchronization is enabled;
- project-owned projections of custom daily `skills` entries when Skill
  synchronization is enabled, excluding official `.system`, caches, temporary
  residue, links, and reparse points, plus the isolated
  `skill-projection-manifest.json` ownership record;
- private copies of `sessions/**/*.jsonl`, `archived_sessions/**/*.jsonl`, and
  `session_index.jsonl` when task snapshot import is explicitly enabled;
- one project-owned `UserPromptSubmit` handler merged into each user
  `hooks.json`, its matching `hooks.state` trust record written through official
  `config/batchWrite`, and the isolated metadata-only
  `session-continuity-manifest.json` when task synchronization is enabled;
- an isolated `goal-intent-sync-manifest.json` and official
  `thread/goal/get|set|clear` calls when optional two-way Goal intent
  synchronization is explicitly enabled; and
- its generated Codex++ external script and exact enablement key only after the
  exact Codex++ host passes compatibility; and
- project-owned renderer-addon settings. Addon source checkouts are read-only
  and remain user-owned.

Provider registration never changes the user's native model, default provider,
model catalog, existing providers, or unknown future settings.
Existing tool configuration is preserved, including unrelated
`shell_environment_policy` entries. The only security merge forces default
exclusions on, adds the provider variable to the exclusion list, and rejects a
configuration that explicitly reintroduces that secret through
`shell_environment_policy.set`.
Provider cleanup retains the shell exclusion because its prior ownership cannot
be proven; leaving a sensitive variable blocked is safer than deleting a
possibly user-owned rule.

## Forbidden Writes

The project must not edit or replace installation directories, executables,
packaged resources, signatures, native launchers, the daily profile, updater
services, update settings, or update channels. An isolated profile or
`CODEX_HOME` must not equal, contain, or be contained by any daily path. The
instance path must not traverse a reparse point. The project must not store
credential values in source, arguments, configuration, logs, reports, tests,
or compatibility evidence. It must not write daily `auth.json`, custom Skills,
task snapshots, arbitrary `config.toml` keys, SQLite/WAL/SHM, logs, Goal database
files, memories, or a renderer-addon source checkout. The session-continuity
exception is limited to its exact handler in daily `hooks.json` and the matching
`hooks.state` record through official config RPC; it stores no prompts, message
bodies, tool output, or project paths. When the user enables Goal intent
synchronization, the only permitted daily Goal mutation is
objective/status/token-budget state through the official app-server RPC; usage
counters remain instance-local. Modified isolated Skill projections are never
written back to the daily source. Divergent session heads are retained and
never collapsed by last-writer-wins.

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
profiles, Skills, tasks, credentials, caches, addon checkouts, and updater
state.
