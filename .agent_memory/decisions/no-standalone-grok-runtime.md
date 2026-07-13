---
type: constraint
status: active
created: 2026-07-14
updated: 2026-07-14
scope: project
paths:
  - README.md
  - docs/ARCHITECTURE.md
  - src/main.rs
verified_by:
  - GitHub API returned no pull requests, deployments, or releases on 2026-07-14
  - Windows process, service, scheduled-task, Appx, uninstall, install-directory, and shortcut audit on 2026-07-14
---
# No Standalone Grok Runtime

## Summary

Grok Build, a standalone Grok desktop/client, Grok CLI, a separate Grok UI,
and a second Grok agent are forbidden dependencies and deployment targets.
The project owner rejects those surfaces on data-security grounds. This is a
project boundary, not a claim that this repository independently verified a
third party's data-handling behavior.

Codex Administrator may expose Grok only as a configured model provider inside
an isolated instance of the official ChatGPT/Codex desktop host. The official
host continues to own the UI, tools, approvals, sandbox, workspace, tasks, and
lifecycle. Provider traffic must go directly to the configured Responses API;
it must never transit through or launch a Grok client.

## Evidence

The 2026-07-14 audit found no Grok/xAI process, service, scheduled task, Appx
package, uninstall entry, common install directory, shortcut, or isolated
injection process. GitHub had no pull request, deployment, or release. The
feature branch contained no tracked references to Grok Build, a Grok client,
a separate Grok UI, or a second agent.

## Use Next Time

Reject any design that introduces, downloads, starts, bundles, or delegates to
standalone Grok software. Do not treat an isolated official ChatGPT/Codex
injection instance as a Grok client: it is still the official host, with only
the model-list and per-task provider route composed by this project.
