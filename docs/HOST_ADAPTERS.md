# Host Adapters

Both adapters deliver the same generated model-list bridge. Neither adapter
owns model execution or the native interface.

## Direct

The direct adapter is reserved for a project-owned isolated instance of the
official desktop application. It may not reuse or activate the daily instance.

A fresh live run of official package `OpenAI.Codex 26.707.9981.0` reconfirmed
that a separate profile and loopback CDP port create a separate process tree.
Starting the same isolated profile a second time with `--new-window` creates an
`app://-/index.html` target on that isolated port. The official
`window.electronBridge` is frozen, sealed, and non-writable; the reviewed hook
therefore composes the writable renderer `postMessage` API discovered from the
same-origin entry bundle instead of replacing the bridge.

The production Direct adapter is implemented. It:

- accepts only packaged `ChatGPT.exe` under a system `WindowsApps` root and
  verifies the suspended image plus official package family before resume;
- derives `profile` and `codex-home` as exact children of one unique
  `CodexAdministrator/instances/<session>` root;
- rejects reparse points anywhere in the instance root's existing ancestor
  chain before creation, configuration writes, or removal;
- creates each process suspended, assigns it to a Windows Job Object, and then
  resumes it;
- passes `--do-not-de-elevate` so Chromium retains the isolated environment
  across its administrator relaunch;
- launches the background process and isolated window in two stages;
- requires the loopback listener PID to belong to its Job Object and validates
  one `app://-/index.html` target on that port;
- waits separately for bridge health and native UI readiness, then requires the
  official app-server `config/read` result to contain `grok_native`;
- installs Ctrl+C handling before any owned path or process is created;
- checks every pre-existing daily PID during maintenance;
- tolerates bounded target/health transitions and reinjects after renderer
  reload while preserving native GPT entries; and
- refreshes descendant lineage during every maintenance pass, then terminates
  only its Job Object plus `(PID, creation time)`-pinned descendants; entry
  ordering is retried, process-open failure is checked against a second system
  snapshot, a still-present inaccessible candidate and snapshot failure remain
  permanent fail-closed uncertainty, and a vanished candidate or PID
  reused after the snapshot becomes a temporary lineage anchor without tracking
  or terminating the replacement; visible descendants become temporary lineage
  members, while main and process-open-recheck PPID edges remain available for
  five seconds. Historical edges preserve topology but do not establish current
  presence; expired state is pruned before export and only current snapshot PIDs
  refresh the window. A current child with
  a known parent PID but ambiguous generation becomes a visible temporary
  anchor/member and is not terminated from that ambiguous edge. The chain
  refreshes the anchor window and causes fail-closed timeout if it persists.
  Shutdown requires five continuous seconds of empty descendant captures and
  anchor observation, rejects backward time within or across snapshots and
  deadline overruns, and deletes only its owned root. Its ten-second absolute
  budget begins before the initial global scan. External broker helpers without
  owned ancestry are included only when a root-contained executable or supported
  Git, PowerShell, or Chromium path argument identifies the exact root. Git `-C`
  resolves cumulatively to the final cwd, drive roots stay absolute, and repeated
  Git path options use their final values. The queried image selects the parser;
  relative path options require a proved final `-C` cwd. Command-line `argv[0]`
  is not image evidence. The first handle has query rights
  only, while termination and
  synchronization use a separate handle with the same creation time; unreadable unmatched
  processes do not widen ownership, while post-match termination/identity failure
  is fail-closed. Query-only liveness uses `GetExitCodeProcess`. Root scan, process wait, and deletion share one separate
  ten-second deadline.

`--no-launch` validates this plan without creating directories or processes.
It uses the same protected system-`WindowsApps` launchability gate as a real
launch; a lookalike fixture path is rejected.
The implementation is not released, merged, or deployed.

## Codex++

The Codex++ adapter uses only these documented external data paths:

```text
%APPDATA%\Codex++\user_scripts\codex-administrator-bootstrap.js
%APPDATA%\Codex++\user_scripts.json
```

It writes the generated bridge atomically and enables only
`user:codex-administrator-bootstrap.js`. Existing scripts and unknown JSON
fields are preserved. Removal deletes only that file and key.

The adapter is enabled only when the executable SHA-256 appears in the shipped
compatibility manifest with matching project, bootstrap, and E2E evidence
identities. Otherwise any stale project script is removed and Codex++ remains
native.

## Update Behavior

For Codex++, an upstream update changes the executable identity and disables
the adapter until review. Direct uses the protected Windows package location,
created-process image and package family, listener ownership, and runtime
target, bridge, UI, native provider, and isolation gates; an incompatible
update fails one of those gates and causes exact cleanup. Neither path blocks,
replaces, pins, or modifies the publisher update.
