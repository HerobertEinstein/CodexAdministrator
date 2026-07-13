# Security Policy

## Supported Versions

No production version has been released. Security reports for the current
default branch are welcome.

## Reporting

Use GitHub private vulnerability reporting for issues that could expose a
credential, alter another provider, execute code outside the declared host
adapter, or modify an official installation.

## Trust Boundary

- Credential values remain in user-managed environment variables.
- Provider configuration stores only the environment-variable name.
- Generated scripts contain model metadata and routing logic only.
- Official installation and updater files are never modified.
- GPT messages and native model entries are preserved unchanged.
- Codex++ injection requires an exact reviewed executable identity.
- Unknown host versions fail closed and remove only stale project-owned data.
- The project does not claim unverified model capabilities.
