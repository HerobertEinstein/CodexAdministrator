# Contributing

Changes to provider configuration, model-list injection, task routing, host compatibility, or update isolation require tests that exercise both success and failure paths.

Do not commit proprietary binaries, credentials, session transcripts, generated authentication capabilities, or copied source from official applications.

Before every push, pull request, merge, tag, or release, audit the complete
public tracked surface against the verified implementation. Update, remove, or
clearly supersede stale README/docs, examples, compatibility metadata, scripts,
assets, CI/release metadata, and generated artifacts in the same change. If the
audit cannot be completed, keep the work unpublished.

Use test-first development for behavior changes and run:

```powershell
cargo fmt --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo build --release --locked
node --test tests/*.test.mjs
```
