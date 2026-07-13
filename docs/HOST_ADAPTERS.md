# Host Adapters

Both adapters deliver the same generated model-list bridge. Neither adapter
owns model execution or the native interface.

## Direct

The direct adapter is reserved for a project-owned debugging connection to the
official desktop application. It is disabled in the current alpha because a
real desktop E2E run has not yet established a safe startup and reinjection
path. Calling it returns an error before touching an official process or file.

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
