# Update Isolation

Codex Administrator is an external launcher and companion. Its update-isolation
contract is stronger than its compatibility contract: an upstream update may
temporarily disable project injection, but Codex Administrator must not modify,
replace, delay, pin, or roll back that update.

## What The Project Guarantees

- Official ChatGPT/Codex and Codex++ installation directories, executables,
  packages, signatures, updater services, update settings, and update channels
  remain owned by their publishers and are never patched by this project.
- The launcher does not block an official application from starting because a
  Codex Administrator component, compatibility check, or injection failed.
- Host injection is allowed only when every host-side executable identity
  required by the selected adapter has an exact SHA-256 match in the shipped
  compatibility manifest and the matching entry has accepted E2E evidence.
- A missing, unreadable, or unknown host identity disables all project
  injection for that launch. The launcher then starts the official host in
  `native_gpt_main` without the project bootstrap.
- Direct and Codex++ adapters never inject into the same host instance.
- Removing Codex Administrator removes only project-owned data outside the
  official installations. It never removes or restores official application
  files, caches, profiles, credentials, conversations, or updater state.

These guarantees protect upstream installation and update autonomy. They do
not promise that project injection remains available immediately after an
upstream release.

## Launch Gate

The fail-closed launch sequence is:

1. Resolve every official host-side executable required by the selected
   adapter without changing it.
2. Read each required executable and calculate its SHA-256 digest.
3. Match the adapter, binary roles, products, architectures, version metadata,
   and SHA-256 digests against one compatibility-manifest entry.
4. Verify that the entry names the exact project/bootstrap versions and an
   accepted E2E evidence SHA-256 digest.
5. Enable `grok_injected_main` only after every check succeeds.
6. On any mismatch or error, skip CDP/bootstrap injection and continue with an
   unmodified `native_gpt_main` launch.

Version strings are diagnostic metadata, not trusted identities. A missing
required binary or a matching version label with a different SHA-256 digest is
an unknown identity and must fail closed. Detection never auto-enrolls a new
digest into the manifest.

## Ownership Boundary

Codex Administrator may own its launcher, companion, configuration, logs,
compatibility manifest, evidence metadata, caches, and namespaced bootstrap
artifacts in project-controlled locations. An optional Codex++ adapter may
install only the project bootstrap through Codex++'s external data or supported
extension surface; that artifact remains project-owned and must be recorded in
the project's fixed, namespaced artifact set.

The project must not:

- write into an official installation directory;
- replace an official executable, package, launcher, shortcut target, or
  updater;
- disable, defer, intercept, or spoof an official update;
- make official startup depend on a successful injection;
- copy an entire ChatGPT/Codex or Codex++ profile into project storage; or
- claim an unrecognized binary is compatible based on a version string alone.

## Upgrade And Failure Behavior

Official products update independently. After an update changes a protected
binary identity, injection remains disabled until maintainers test that exact
identity, attach the required E2E evidence, and publish a new compatibility
manifest entry. Native startup remains the fallback during that interval.

If the bootstrap becomes unhealthy after launch, the adapter must dispose only
project-owned UI and state and return to `native_gpt_main`. It must not repair
the situation by patching the host or rolling back the official update.

## Uninstall Boundary

The current Codex++ adapter removes only
`user_scripts/codex-administrator-bootstrap.js` and the exact
`user:codex-administrator-bootstrap.js` configuration key. It preserves every
other script, key, unknown/future field, and the global user-script setting.
Parent application data directories are never recursively removed.

This rule also applies to project bootstrap data stored in a Codex++ external
data directory: remove only the exact project-owned artifact, not Codex++ data
around it. Official binaries and user data remain untouched.

## Verification Requirement

Changes to launcher, adapter, bootstrap, manifest, update, or uninstall logic
must include tests proving the native fallback and ownership boundary. A release
cannot describe a host identity as supported without the compatibility-manifest
entry and E2E evidence defined in [COMPATIBILITY.md](COMPATIBILITY.md).
