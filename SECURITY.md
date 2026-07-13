# Security Policy

## Supported versions

No production version has been released yet. Security reports for the current main branch are welcome.

## Reporting

Do not open a public issue for a vulnerability that could expose credentials, execute commands, or cross the loopback trust boundary. Use GitHub private vulnerability reporting once the public repository is available.

## Trust boundary

- Companion services bind to loopback only.
- Every launch uses a new capability value.
- Control capabilities are never written to logs or browser storage.
- The injected bootstrap is digest-verified and contains no reusable account credential.
- Official Grok and Codex credential stores remain runtime-owned.
- The project never modifies official application installation files.
- Exactly one selected host adapter owns injection for a ChatGPT/Codex instance.
- The optional Codex++ adapter uses an installed external release; no Codex++ source is copied into this project.
- Concurrent writers require separate worktrees or an explicit lease transfer.
