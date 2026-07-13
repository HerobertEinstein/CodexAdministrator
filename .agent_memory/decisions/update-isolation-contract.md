---
type: decision
status: active
created: 2026-07-13
updated: 2026-07-13
scope: project
paths:
  - README.md
  - docs/UPDATE_ISOLATION.md
  - docs/COMPATIBILITY.md
verified_by:
  - git diff --check -- README.md docs .agent_memory
  - local Markdown link resolution check on 2026-07-13
---
# Update Isolation And Compatibility Identity

## Summary

Codex Administrator is external to official ChatGPT/Codex and Codex++
installations. It must never modify or block their installation or update
mechanisms. Injection is permitted only when every host-side executable required
by the selected adapter has an exact SHA-256 identity in the compatibility
manifest with accepted E2E evidence. Any unknown or unreadable required identity
disables project injection and continues with the official host in unmodified
`native_gpt_main`.

Uninstall/fallback deletes only the fixed namespaced project bootstrap and exact
configuration key. Even when the bootstrap lives in a Codex++ external data
directory, its surrounding official/user data must remain untouched.

## Evidence

- `docs/UPDATE_ISOLATION.md` defines upstream ownership, launch fallback, and
  uninstall boundaries.
- `docs/COMPATIBILITY.md` defines exact manifest identity and the required E2E
  evidence matrix.
- `README.md` states the user-facing guarantee and its compatibility limit.

## Use Next Time

For any host, adapter, bootstrap, update, or uninstall change, preserve these
invariants:

1. Never write to or gate an official installation or updater.
2. Never trust a version string in place of every required binary's exact
   SHA-256 identity.
3. Never enable injection without a reviewed manifest entry plus E2E evidence.
4. Always allow unknown identities to start natively without project injection.
5. Delete only the exact namespaced artifacts owned by this project; never
   recursively clean a host data directory.

Treat a request to "survive official updates" as update isolation plus a tested
compatibility allowlist, not as permission to patch upstream files or promise
unverified forward compatibility.

## Related / Supersedes

This entry is the project memory source for update isolation. Architecture and
implementation details may refine mechanisms, but must not weaken these
boundaries without an explicit reviewed decision.
