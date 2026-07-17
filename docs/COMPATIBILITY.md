# Compatibility

Codex++ compatibility is granted to an exact reviewed executable identity and
bootstrap version. Direct compatibility is established from the protected
official package location, the suspended process image and package family, and
live isolation, CDP, bridge, and UI gates. Neither path trusts a product name
alone.

## Host Gate

`compatibility.json` contains reviewed host entries with:

- adapter identity;
- executable SHA-256;
- Codex Administrator version;
- bootstrap protocol version; and
- immutable E2E evidence SHA-256.

An unknown, changed, malformed, or unsupported entry fails closed. For
Codex++, failure removes only this project's stale external script and leaves
the host native. The shipped alpha manifest is intentionally empty.

Direct accepts only packaged `ChatGPT.exe` under a system `WindowsApps` root.
Before resuming each created process it verifies the actual image path and the
official `OpenAI.Codex_2p2nqsd0c76g0` package family. It then requires the CDP
listener PID to belong to its Job Object and applies target, bridge, UI, and
native provider-readiness gates. An update that changes those contracts fails
closed and cleans the isolated instance without modifying the package.

## Required Desktop Evidence

Before a host identity is accepted, a fresh desktop run must prove:

1. a separate profile, `CODEX_HOME`, process tree, and loopback CDP port are
   established without touching the daily instance;
2. the daily root instance remains alive before, during, and after injection;
3. the native interface starts without modified installation files;
4. the frozen official bridge retains the exact same object and function
   identity;
5. every native GPT entry remains present and unchanged;
6. each configured Grok entry appears once;
7. selecting GPT emits the original request object;
8. selecting Grok routes only new Grok tasks to `grok_native`;
9. when resume compatibility is claimed, a known Grok task resumes through the
   same provider;
10. the official app-server `config/read` response contains
    `model_providers.grok_native` before the launcher reports ready;
11. disposal restores the exact prior writable renderer API function; and
12. an incompatible update leaves the host native.

The most recent retained Direct E2E, on 2026-07-17 with
`OpenAI.Codex 26.715.2305.0`, used configured mode with exact model
`grok-4.5`. It proves automatic executable discovery, suspended
official-package acceptance, listener PID ownership, bridge and UI readiness,
an eight-model reviewed refresh, native-menu Grok selection, one exact native
text response through `grok_native`, daily-instance preservation, and zero
owned process/profile/port residue. A separate run on the same package retains
management-only proof with no selected model or provider credential. Scoped CDP tests prove
startup reinjection after a renderer reset. Separate Windows tests prove
runtime-persistent escaped-descendant lineage tracking, orphan discovery after
an intermediate parent exits, multi-generation PID tracking, entry-order retry,
generation-time rejection of reused or post-snapshot PIDs, a second-snapshot
presence check after process-open failure, permanent fail-closed uncertainty
only for candidates still present but inaccessible, temporary lineage
anchors for vanished or reused PIDs, transitive observation for every visible
descendant of those anchors, termination of already tracked handles when process
snapshots fail, persistent reporting of transient snapshot failures, five
continuous seconds of empty descendant captures, strict deadline enforcement,
backward-clock rejection, and a late file-release retry beyond the former
two-second limit. A visible anchored chain refreshes the observation window and
promotes its visible descendants to temporary lineage members. That preserves
taint across an intermediate member's exit. PPID edges from main snapshots and
process-open rechecks remain available for the same five-second window, so the
next capture can reconnect a surviving grandchild through an exited member.
Historical edges do not prove presence: only current main/recheck snapshot PIDs
refresh the window, and expired state is pruned before capture input export. A known-parent child with ambiguous generation is retained as
a visible temporary anchor/member without being terminated from that relation. A
persistent chain causes fail-closed timeout; a chain that disappears is not a
permanent error by itself. Replacement processes are never tracked or
terminated. Snapshot FILETIME must remain monotonic across main and recheck
captures. This is bounded repeated-snapshot lineage monitoring, not a kernel
process-creation trace. These gates do not prove endpoint feature parity.

A separate 2026-07-18 local owner deployment gate refreshed eight reviewed
Grok models, imported all 323 current daily task snapshots into the retained
private home while preserving nine pre-existing isolated snapshots, and passed
two managed restart cycles. The launcher settings hash and native `[desktop]`
settings remained unchanged, all nine daily ChatGPT PID/creation-time/image
identities remained present, and no launcher error was produced. This proves
deployment persistence and one-way incremental import, not arbitrary-history
resume or complete capability parity.

An external broker can start official plugin-sync helpers without an owned PPID
chain. Direct therefore queries process command lines through the native Windows
process-information API during shutdown and before each root-removal attempt.
Only root-contained executables or supported Git, PowerShell, and Chromium
path-argument syntax can match; arbitrary executables, `--`, and command/message
payloads cannot. Multiple Git `-C` options resolve to one final cwd, while
drive roots remain absolute and repeated Git path options use their final value.
Relative Git path options require a proved final `-C` cwd; an unknown initial cwd
does not match.
The queried image selects the parser; command-line `argv[0]` is not image
evidence. The first handle requests query rights only; termination plus
synchronization use a separate handle, and both
must report the same creation time. Unreadable processes without an exact match
do not widen ownership; termination-right or identity failure after a match is
fail-closed. Query-only liveness uses `GetExitCodeProcess`, not exit-time fields.
Shutdown and Drop start a thirty-second descendant cleanup budget after the
initial global scan; root scanning, process wait, and deletion retain a
separate 10-second deadline.

A configured-provider run on the current package used exact model `grok-4.5`,
selected it through the official model menu, returned one exact text response,
and recorded `modelProvider = grok_native`. Separate retained runs on an
earlier dated package completed one `update_plan` function-call/output loop and
one native shell `commandExecution` with exit code `0`. Each run preserved the
pre-existing daily process identities and removed its exact owned listener,
process tree, and test root.

Message-level tests are necessary but do not satisfy this desktop gate.

## Provider Gate

Provider registration requires a valid Responses endpoint, a secure remote
scheme, a `/v1` path, and a valid environment-variable name. Failure occurs
before configuration changes. Credential values are outside every persisted
artifact and report. A Direct launch independently confirms that the official
app-server loaded `grok_native`; a file write or healthy renderer alone is not a
readiness signal.

## Capability Claims

Model-list success is not feature parity. Each exact model and endpoint needs
separate evidence for text streaming, tools, parallel tools, files, images,
structured output, reasoning controls, cancellation, resume reliability, and
any additional native feature. Unsupported or unknown behavior remains
unclaimed without changing the host's existing providers. For exact
`grok-4.5` on `OpenAI.Codex 26.715.2305.0`, public Responses streaming and one
native app-server text turn have passed. Earlier dated packages separately
prove one `update_plan` function-call/output loop and one native shell
`commandExecution` loop. Files, images, parallel tools, structured output,
cancellation, resume reliability, and complete parity remain unclaimed on every
package. Transient upstream availability errors are operational evidence, not
durable compatibility claims.
