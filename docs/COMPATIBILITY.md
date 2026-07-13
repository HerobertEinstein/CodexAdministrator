# Compatibility

Compatibility is granted to an exact host executable and bootstrap version,
not inferred from a product name or a similar release number.

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
10. disposal restores the exact prior writable renderer API function; and
11. an incompatible update leaves the host native.

Message-level tests are necessary but do not satisfy this desktop gate.

## Provider Gate

Provider registration requires a valid Responses endpoint, a secure remote
scheme, a `/v1` path, and a valid environment-variable name. Failure occurs
before configuration changes. Credential values are outside every persisted
artifact and report.

## Capability Claims

Model-list success is not feature parity. Each exact model and endpoint needs
separate evidence for text streaming, tools, parallel tools, files, images,
structured output, reasoning controls, cancellation, resume reliability, and
any additional native feature. Unsupported or unknown behavior remains
unclaimed without changing the host's existing providers.
