# Compatibility

Compatibility is capability-specific evidence, not an assumption inferred
from a provider name, model label, endpoint response, or successful model-list
request. The official ChatGPT/Codex host remains the capability authority.

## Provider Configuration Gate

Provider registration is allowed only when all configuration checks pass:

- the wire API is `responses`;
- the base URL is syntactically valid and ends in `/v1`;
- remote endpoints use HTTPS, with HTTP limited to loopback development;
- `env_key` is a valid uppercase environment-variable name; and
- no credential value is present in persisted configuration or evidence.

When a reviewed model catalog is supplied, it must be a bounded JSON file with
a non-empty `models` array, the required official model-entry fields, and the
exact selected model slug. Catalog metadata does not override capability
evidence; it is the official host input that reflects already-reviewed
evidence.

Invalid configuration fails closed: the Grok provider is not registered or
updated, existing configuration is preserved, and official host startup is not
blocked.

## Capability Decision Table

| Observed state | Grok capability claim | Official host behavior |
| --- | --- | --- |
| Model identifier is visible but no capability evidence exists | Unsupported | Continue unchanged |
| Endpoint or authentication validation fails | Provider unavailable | Continue with existing providers |
| Responses text and streaming pass through the official host | Allow only those validated behaviors | Keep native controls |
| A tool, approval, sandbox, workspace, session, or cancellation behavior is unknown or differs | Unsupported for that behavior | Do not bypass or emulate it |
| Complete capability-specific E2E evidence is accepted | Allow only the exact validated model/provider/capability combination | Continue under host ownership |

No CLI probe, ACP handshake, version similarity, model-list result, or one-off
successful prompt may bypass this table.

## Evidence Matrix

Every published capability claim must identify the official host version, OS,
architecture, provider endpoint identity, model identifier, project revision,
test implementation, result, and immutable evidence digest. Evidence should
cover the exact capability being claimed, including failure behavior.

The machine-readable manifest schema binds the evidence digest to
`grok_native`, one or more exact model identifiers, and explicit booleans for
Responses, streaming, tool calls, parallel tool calls, image input, file input,
structured outputs, reasoning summaries, and WebSockets. Unknown fields are
rejected and omitted capabilities remain disabled.

The matrix treats these as separate claims:

1. Provider configuration without persisted secrets, plus explicit reversible
   model selection for native desktop launch.
2. Authentication failure with the official host remaining usable.
3. Text response and streaming behavior through the official host.
4. Tool-call request, approval, execution, and result round-trip.
5. Sandbox and workspace containment with no provider-side bypass.
6. Cancellation, retry, resume, and session-history behavior.
7. Structured outputs, image input, and reasoning metadata when advertised.
8. Provider disablement or removal preserving unrelated user configuration.

A screenshot, model-list result, or undocumented manual assertion is
insufficient.

The repository's live Codex mock-provider tests currently cover configuration,
thread selection, Responses SSE text streaming, one host-owned function-tool
round trip, and local PNG input conversion. Real Grok endpoint authentication,
model-specific reasoning, shell/exec-server environments, approvals,
cancellation, structured outputs, and every additional modality remain separate
evidence rows.

## Model Visibility Is Not Parity

Different models exposed by one provider can have different capabilities. A
visible Grok identifier does not inherit the tools, permissions, context,
modalities, structured-output behavior, or reliability of another Grok or GPT
model. Aliases and newly discovered identifiers start with no capability claims
until separately validated.

## Compatibility Limits

Codex Administrator cannot guarantee undocumented provider or host behavior
across upstream changes. Unknown behavior fails closed at the provider or
capability boundary; it must not trigger a Grok CLI/ACP fallback or changes to
official installations.

Legacy compatibility manifests and evidence for Grok UI injection, Grok Build,
or ACP are historical artifacts only. They do not establish active support for
the canonical native-provider route.
