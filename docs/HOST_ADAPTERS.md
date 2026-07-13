# Host Adapters

Both adapters deliver the same generated model-list bridge. Neither adapter
owns model execution or the native interface.

## Direct

The direct adapter is reserved for a project-owned isolated instance of the
official desktop application. It may not reuse or activate the daily instance.

A read-only probe of official package `OpenAI.Codex 26.707.8479.0` established
that a separate profile and loopback CDP port create a separate process tree.
Starting the same isolated profile a second time with `--new-window` creates an
`app://-/index.html` target on that isolated port. The official
`window.electronBridge` is frozen, sealed, and non-writable; the reviewed hook
therefore composes the writable renderer `postMessage` API discovered from the
same-origin entry bundle instead of replacing the bridge.

The direct adapter remains disabled because the production launcher, process
ownership monitor, reinjection monitor, and cleanup lifecycle are not yet
implemented. Calling it returns an error before touching any daily profile,
process, CDP target, or official file.

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

An upstream update changes the executable identity and therefore disables the
adapter until that release is reviewed. The update itself is never blocked,
replaced, pinned, or modified.
