# Compatibility

Compatibility is an explicit allowlist, not an assumption inferred from an
upstream product name or version label. The separate
[update-isolation contract](UPDATE_ISOLATION.md) protects official installation
and update autonomy even when no compatible injection is available.

## Compatibility Identity

Every supported combination has one immutable manifest entry containing at
least:

- host adapter: `direct` or `codexplusplus`;
- the executable's exact SHA-256 digest used as its binary identity;
- the exact Codex Administrator project version;
- the exact bootstrap contract version; and
- the accepted E2E evidence SHA-256 digest.

Every SHA-256 digest required by the selected adapter is mandatory. Publisher
and version metadata cannot replace one. Entries are not wildcarded across
binary digests, host adapters, product channels, or architectures. Runtime
identities for Grok Build and Codex app-server are gated independently from the
desktop host identity.

## Decision Table

| Observed state | Project injection | Official host launch |
| --- | --- | --- |
| Every required identity exactly matches the manifest and accepted E2E evidence | Allowed for the listed adapter and capabilities | Continue |
| Any required binary is missing or has an unknown/changed SHA-256 digest | Disabled | Continue in unmodified `native_gpt_main` |
| Identity probe cannot read or hash the binary | Disabled | Continue in unmodified `native_gpt_main` |
| Manifest or evidence is missing, invalid, or revoked | Disabled | Continue in unmodified `native_gpt_main` |
| Bootstrap health fails after injection | Dispose project UI and disable injection | Restore/continue `native_gpt_main` |

No network lookup, version similarity, or successful one-off mount may bypass
this table. A host update cannot be blocked while maintainers prepare a new
entry.

## E2E Evidence Matrix

A compatibility entry requires reproducible evidence from a clean installation
of the exact binary identity. At minimum, the matrix covers:

1. Official host launch and native GPT use with injection disabled.
2. Unknown-identity fallback with no bootstrap, CDP mutation, or startup block.
3. Grok UI mount, mode switching, disposal, and complete official UI
   restoration.
4. Renderer navigation, target recreation, host restart, and project restart.
5. Grok session create/resume/cancel and Codex app-server subtask execution.
6. Approvals, cancellation, workspace/worktree ownership, and process-tree
   cleanup.
7. Bootstrap or companion failure after launch and recovery to
   `native_gpt_main`.
8. Uninstall against a populated official profile, proving that only
   ledger-recorded project artifacts are removed.

Evidence must identify the OS build, architecture, adapter, all required binary
SHA-256 digests, project commit/release, test implementation, result, and
immutable artifact digest. A screenshot alone or an undocumented manual
assertion is insufficient.

The alpha `compatibility.json` intentionally has no accepted host entries. A
locally detected digest is never auto-enrolled.

## Manifest Governance

- New identities enter the manifest only through review of the full matrix.
- A changed required binary digest requires a new entry and new evidence even
  when its version string is unchanged.
- Failed or vulnerable combinations are revoked; revocation disables project
  injection without changing the official installation.
- Local manifest edits do not constitute published support.
- CI schema checks are necessary but do not replace Windows E2E evidence.

## Compatibility Limits

Codex Administrator cannot guarantee that an unannounced upstream change will
preserve undocumented DOM, Electron, process, CDP, user-script, ACP, or
app-server behavior. The project guarantees fail-closed isolation and native
startup for unknown identities; it guarantees injection compatibility only for
the exact combinations backed by a published manifest entry and accepted E2E
evidence.
