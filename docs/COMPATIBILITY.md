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
9. a known Grok task resumes through the same provider;
10. the official app-server `config/read` response contains
    `model_providers.grok_native` before the launcher reports ready;
11. disposal restores the exact prior writable renderer API function; and
12. an incompatible update leaves the host native.

The latest Direct E2E on `OpenAI.Codex 26.707.9981.0` proves automatic
executable discovery, suspended official-package acceptance, listener PID
ownership, bridge and UI readiness, native provider readiness, daily-instance
preservation, and zero owned process/profile residue. Scoped CDP tests prove
startup reinjection after a renderer reset. Separate Windows tests prove
escaped-descendant lineage cleanup and a late file-release retry beyond the
former two-second limit. These gates do not prove endpoint feature parity.

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
unclaimed without changing the host's existing providers. Three final direct
Responses probes returned HTTP 503, so no complete text or tool capability is
currently claimed.
