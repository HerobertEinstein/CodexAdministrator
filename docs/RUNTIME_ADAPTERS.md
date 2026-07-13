# Native Host And Model Provider Boundary

## Official Host Authority

The official ChatGPT/Codex host is the runtime. It owns prompts, tools,
approvals, sandbox enforcement, workspace access, sessions, compaction,
credentials, cancellation, and errors. Codex Administrator does not recreate
or proxy those capabilities through another agent process.

The project does not launch Grok CLI, Grok Build, or an ACP process as the main
agent. It also does not launch a separate Codex app-server in order to give
Grok its own agent loop. Host-native behavior must stay on the official host's
supported execution path.

## Grok Responses Provider

Grok is integrated only as a Responses-compatible model provider. The
supported configuration shape is equivalent to:

```toml
[model_providers.grok_native]
name = "Grok in native ChatGPT/Codex"
base_url = "https://gateway.example/v1"
env_key = "GROK_NATIVE_API_KEY"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
```

`env_key` is an uppercase environment-variable name. The secret itself must be
supplied by the environment and must never be written to configuration, logs,
receipts, evidence, or source control. Provider registration preserves
unrelated configuration. Explicit Grok launch selects the requested model
through the official persisted configuration surface.

Remote provider URLs must use HTTPS and end in `/v1`. Plain HTTP is allowed
only for loopback development endpoints, and query strings are rejected.
WebSocket support is disabled unless the endpoint and official host path have
both been explicitly validated.

The launcher selects the provider through the official user configuration:

```toml
model_provider = "grok_native"
model = "<model>"
model_catalog_json = "<reviewed-catalog>"
```

It then invokes `codex app <workspace>`. Official Codex 0.142.3 discards root
`-c` overrides in the `app` dispatch path, so treating those flags as desktop
selection would be a false integration claim.

For an npm Codex installation, the launcher calls `node.exe` plus the absolute
official `@openai/codex/bin/codex.js` entrypoint directly. It does not execute a
shell wrapper. The API key value is inherited from the named environment
variable and is not added to the process arguments.

The model catalog remains a project-owned evidence artifact outside official
installations. It must contain the selected Grok slug, satisfy the required
official model-entry shape, and advertise only capabilities already accepted
for that exact model/provider combination. The launcher additionally requires
the installed official Codex runtime to parse it successfully through `debug
models` before changing the active selection.

The previous native selection is stored in a credential-free sidecar. Repeated
Grok launches preserve the original backup. `launch-native` restores it only
when the current managed fields still match, so a later manual user choice is
never overwritten.

## Fail-Closed Validation

A model appearing in discovery or a selector proves only identifier
visibility. Before a capability is described as supported, evidence must show
that the exact model/provider combination completes it through the official
host while retaining host-owned approvals, sandbox, workspace, and session
semantics.

Validation is capability-specific. At minimum, streaming, tool calls,
structured outputs, image input, cancellation, resume, and reasoning metadata
are independent claims. Unknown events, fields, or behavior do not inherit
support from another model.

The code-level capability contract defaults every field to disabled. Native
Codex agent readiness requires verified Responses, streaming, and tool calls;
multimodal readiness separately requires verified image and file input.

If endpoint, authentication, protocol, model, or capability validation fails:

- do not expose the failed capability as supported;
- do not fall back to Grok CLI, Grok Build, ACP, or a custom tool runner;
- do not weaken approvals, sandboxing, or workspace restrictions; and
- leave the official host and existing providers operational.

## Current Automated Proof

Against installed official Codex 0.142.3, isolated live tests now prove:

- strict custom Responses provider configuration and `thread/start` model
  selection;
- SSE text deltas reaching `item/agentMessage/delta`;
- an `update_plan` function call executed by the official host, with matching
  `function_call_output` returned in the second Responses request; and
- a valid local PNG converted by the official host into Responses
  `input_image` content.

These tests use a loopback mock provider and dummy environment credential. They
prove the host/provider contract, not that a particular xAI/Grok endpoint has
passed it. Standalone app-server shell/workspace execution additionally depends
on an official exec-server environment and remains a separate E2E gate.

## Historical Note

The repository previously explored direct child-process adapters for Grok ACP
and Codex app-server, including Grok Build support claims. That architecture is
superseded and must not be presented as an active route. Historical references
are useful only when clearly marked as rejected context.
