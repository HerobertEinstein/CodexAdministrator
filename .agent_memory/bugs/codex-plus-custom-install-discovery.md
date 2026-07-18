---
type: bug
status: active
created: 2026-07-18
updated: 2026-07-18
scope: project
paths:
  - src/main.rs
  - tests/cli_contract.rs
  - Cargo.toml
  - compatibility.json
verified_by:
  - cargo test --test cli_contract live_registered_custom_codex_plus_install_is_discovered -- --ignored --exact
  - GitHub release v1.2.34 asset digest 277374908ee59d7b4dc86f037502b7518f41b476c551c45123a71dec15a140a2
  - installed launcher SHA-256 4364D88A23FD2C3DC73B7FC9C4AC946EFDDC774D96FEDAE03C523DE8C2611E32
---
# Codex++ Custom Install Discovery

## Summary

The original doctor probe checked only `CODEX_PLUS_PLUS_PATH`, Program Files,
and `%LOCALAPPDATA%/Programs`. It incorrectly reported a valid custom-path
Windows installation as not found. Discovery now also reads the publisher's
`CodexPlusPlus` uninstall registration and keeps `found` separate from
`eligible`.

## Evidence

The owner installation is Codex++ `1.2.34`. Its launcher and manager match the
official GitHub release ZIP byte-for-byte. The shipped compatibility manifest
remains empty because official asset identity is necessary but not sufficient:
separate profile, `CODEX_HOME`, process ownership, daily-instance preservation,
and user-script composition E2E are still required.

## Use Next Time

Never interpret `doctor found=false` as proof that Codex++ is absent without
checking discovery coverage. Never interpret `found=true` as permission to
inject. Preserve the three states independently: installed, discovered, and
eligible.
