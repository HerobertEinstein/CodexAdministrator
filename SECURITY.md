# Security Policy

## Supported Versions

No production version has been released. Security reports for the current
default branch are welcome.

## Reporting

Use GitHub private vulnerability reporting for issues that could expose a
credential, alter another provider, execute code outside the declared host
adapter, or modify an official installation.

## Trust Boundary

- The native selector manager stores the provider key only as a per-user
  Generic Credential in Windows Credential Manager. The CLI alternative keeps
  it in a user-managed environment variable.
- The stored credential includes an endpoint fingerprint. A different Base URL
  or Action Path cannot reuse it after an offline settings change or restart.
- Provider configuration stores only the environment-variable name. The
  isolated child receives the key through a project-specific process
  environment variable that native shell tools are configured to exclude.
- Generated scripts contain model metadata, routing, the native-selector
  manager bridge, and optional exact-hash renderer addon payloads. They contain
  no provider credential or copied daily authentication state.
- Both launcher stages remove recognized credential-bearing and secret-shaped
  inherited environment variables, including common API-key, token, password,
  PAT, connection-string, and database credential names. A configured official
  child receives only the explicit project provider key among variables
  classified as sensitive; management-only receives none. Custom credentials
  with non-secret-looking variable names must be unset before launch.
- Native authentication, custom Skills, and optional task snapshots are copied
  one-way into a project-owned isolated home. Daily state is never modified or
  shared in place by those copy flows.
- Goal intent synchronization is disabled by default. If the user opts in, the
  launcher discovers the native binary from an installed official npm Codex
  package, disables plugins and apps in its short-lived app-server helpers,
  scrubs secret-shaped environment variables, and uses only
  `thread/goal/get|set|clear`. It never copies Goal SQLite files. One-sided
  objective/status/token-budget changes may update the other home through the
  official API; divergent or concurrently changed destinations remain
  conflicts, and token and elapsed-time counters remain instance-local.
- Hard-linked `auth.json` and task snapshots are rejected, along with reparse
  points and other shared-path aliases that could break copy isolation.
- Hard-linked custom Skill files are excluded. The official `.system` tree,
  caches, temporary residue, symlinks, junctions, and reparse points are not
  projected. `skill-projection-manifest.json` permits updates or removals only
  while the isolated destination still matches its prior hash. Modified
  isolated Skill projections are never written back to the daily source.
- Official installation and updater files are never modified.
- GPT messages and native model entries are preserved unchanged.
- Codex++ injection requires an exact reviewed executable identity.
- Unknown host versions fail closed and remove only stale project-owned data.
- The project does not claim unverified model capabilities.

## Local DevTools Boundary

The Chromium DevTools endpoint is unauthenticated. Direct mode keeps it on a
random loopback port only for the lifetime of the isolated instance, verifies
that the listener PID belongs to the owned Job Object, and validates the exact
renderer target before use. Those checks prevent accidental attachment to the
wrong host; they do not authenticate other local clients.

A hostile local process, or another local Windows account able to discover and
connect to that loopback endpoint, is outside this threat model. Such a client
could inspect or manipulate the isolated renderer, including transient manager
form input. Do not run Direct mode alongside untrusted local software. The
listener never binds beyond loopback and is verified closed during shutdown.
