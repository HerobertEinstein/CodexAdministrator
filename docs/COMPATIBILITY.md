# Compatibility And Update Isolation

Codex Administrator cannot control whether an external product changes an undocumented UI or integration surface. It instead provides an enforceable update-isolation contract.

## Guarantees

- Official ChatGPT/Codex and Codex++ files, packages, update settings, and updaters are never modified.
- Direct and Codex++ host adapters are mutually exclusive for one host instance.
- Every host adapter reports an exact observed host version/build identity before injection is enabled.
- Only versions present in the verified compatibility manifest may enter `grok_injected_main`.
- Missing or unknown versions fail closed to `native_gpt_main` while leaving the official host usable.
- Bootstrap health is checked after injection, navigation, target recreation, and restart.
- Project data and injected UI can be disabled or removed without rolling back an official application update.
- Grok Build and Codex app-server adapters are version-gated independently from the host adapter.

## Non-guarantees

No independent project can guarantee that an unannounced upstream change will preserve an undocumented DOM, process, or CDP behavior. Codex Administrator therefore never treats a newly observed version as compatible before its contract and E2E matrix pass.

## Release gate

A compatibility entry requires:

1. Clean installation of the official host version.
2. Native GPT launch with injection disabled.
3. Grok UI mount, mode switching, disposal, and official UI restoration.
4. Renderer navigation and target recreation recovery.
5. Grok session create/resume/cancel and Codex app-server subtask execution.
6. Approval, worktree ownership, restart, and failure-path tests.

Compatibility entries identify the adapter, exact host build, bootstrap version, companion version, and E2E evidence digest.
