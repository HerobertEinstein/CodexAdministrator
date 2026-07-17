---
type: constraint
status: active
created: 2026-07-17
updated: 2026-07-17
scope: project
paths:
  - README.md
  - docs/
  - .github/
  - compatibility.json
  - assets/
  - src/
  - tests/
verified_by:
  - full tracked-tree publication audit against current source and retained evidence
  - git diff --check
  - credential and forbidden-runtime scans
---
# GitHub Publication Freshness Gate

## Summary

The public GitHub repository is a product surface, including public feature
branches. No push, pull request, merge, tag, or release may knowingly leave
tracked material that presents an obsolete architecture, capability,
compatibility result, setup path, runtime dependency, test result, or release
state as current.

Historical material may remain only when it is clearly dated or archived and
cannot be mistaken for current guidance. Unverified capability must remain
explicitly unverified. In particular, Codex++ compatibility stays disabled
until the exact host build passes its gate, and no Grok Build, standalone Grok
client, second UI, or second Grok agent may appear as a supported dependency.

## Trigger

Run this gate before every GitHub push, pull request, merge, tag, release, or
public documentation update.

## Forbidden Route

Do not push the implementation first and defer stale README, docs, examples,
compatibility metadata, scripts, assets, CI/release metadata, or generated
artifacts to a later cleanup. Do not treat a feature-branch push as private.

## Minimum Safe Action

Audit the complete tracked tree against current source and retained evidence;
update, remove, or clearly supersede every misleadingly stale item in the same
publication change. Re-run the repository, credential, forbidden-runtime,
formatting, and test gates. If the audit cannot be completed, keep the work
unpublished. For the active slice, push only
`feat/isolated-injection-instance`; do not modify `main`.
