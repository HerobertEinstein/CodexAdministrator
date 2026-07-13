# Contributing

Changes to launcher, injection, authentication, process execution, session mapping, approvals, or workspace ownership require tests that exercise both success and failure paths.

Do not commit proprietary binaries, credentials, session transcripts, generated authentication capabilities, or copied source from official applications.

Use test-first development for behavior changes and run:

```powershell
cargo fmt --check
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
```
