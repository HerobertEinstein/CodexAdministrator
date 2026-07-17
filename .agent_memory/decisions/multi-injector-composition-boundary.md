---
type: decision
status: active
created: 2026-07-16
updated: 2026-07-17
scope: project
paths:
  - src/renderer_addons.rs
  - src/startup.rs
  - assets/bootstrap.js
  - assets/model-picker-mount.js
  - assets/renderer-addon-runtime.js
  - compatibility.json
  - renderer-addons.json
  - docs/ARCHITECTURE.md
verified_by:
  - read-only Codex++ audit at commit 23bbf134d051f239de73a6b2ebea0abc36649986
  - read-only Codex Dream Skin audit at commit 568469a4f97e8fa4c8d237ce018c206c29959ecd
  - cargo test --all-targets --locked
  - node --test tests/*.test.mjs
---
# Multi-Injector Composition Boundary

## Summary

Codex Administrator must support compatible injectors without becoming the
exclusive renderer owner and without placing unrelated launchers inside the
Direct Job Object. Exactly one component owns each official ChatGPT/Codex
instance, profile, `CODEX_HOME`, process tree, CDP listener, and cleanup lease.

- In Direct mode, Codex Administrator is the sole instance owner.
- In Codex++ mode, Codex++ remains the instance owner and Codex Administrator
  enters only through the reviewed `user_scripts` surface.
- Visual projects such as Codex Dream Skin are optional renderer payloads. The
  shipped manifest currently permits them only on Direct. A future Codex++ host
  requires its own exact host and composition E2E before any addon is enabled;
  addons never become process owners or receive permission to restart or kill
  other ChatGPT trees.

Codex++ is not launched inside the Direct Job Object and is not treated as a
second Direct-side renderer bridge. Supporting Codex++ and a skin at the same
time means: a reviewed Codex++ host loads the Administrator user script, then a
reviewed visual payload is added without touching Codex++ binaries or official
files. The only Codex++ write exception is its documented extension slot, where
Administrator owns exactly one script file and one configuration key while
preserving all other scripts and fields.

No Codex++ identity is eligible while the shipped `compatibility.json` host
list is empty. Source familiarity, a local installation, or a prior version
audit is not compatibility evidence for a new executable identity. Eligibility
requires an exact binary hash plus host/composition E2E. Compatibility fallback
removes only stale Administrator-owned extension residue and must not launch an
unverified host.

## Codex Dream Skin Boundary

The audited Windows source at commit
`568469a4f97e8fa4c8d237ce018c206c29959ecd` is a loopback-CDP theme. Its current
install/start/restore scripts are forbidden integration surfaces because they
modify `$HOME/.codex/config.toml`, use one global state file, can stop all
`ChatGPT` processes when `-RestartExisting` is used, and do not isolate
`CODEX_HOME`.

Administrator may only read a user-supplied checkout and compose the reviewed
renderer assets through the already owned target. It must not copy those assets
into this repository or release package, run the upstream install/start/restore
scripts, create its shortcuts, start its Node daemon, or write its global state.
The current reviewed asset SHA-256 values are:

- `windows/assets/renderer-inject.js`:
  `d17514772b5b35f48d15e7e79bc3957461e10aefa401b20c4d9cfa5392cb656f`
- `windows/assets/dream-skin.css`:
  `a85bd61d699496928ab19a5d9ea6d1aaaaeedd4bbfb48e37d873854480b53fff`
- `windows/assets/dream-reference.png`:
  `e6019a268915194e270d9ad4eb44d99c1a43c22c11463137147d9e00428375fc`

The root repository has no root-level license file and GitHub does not identify
a repository license; only `macos/LICENSE` contains MIT text while the README
points to it. Until upstream clarifies Windows coverage, do not embed or
redistribute Windows assets. Unknown or changed asset hashes disable only the
skin and leave the official host plus Administrator healthy.

## Composition Contract

Injection order is host bridge, Administrator bootstrap, then optional renderer
payloads. The schema-v2 manifest provides a generic external entrypoint and
typed asset substitutions instead of Dream-specific fields. Planning is stable
by `load_order` then ID, host-scoped, and conflict-aware. The first successful
payload owns an exclusive slot; later conflicts are disabled with an explicit
blocker. Cleanup runs in reverse. Every payload needs a namespaced identity,
idempotent apply, bounded health probe, and exact dispose path. Optional payload
failure is fail-closed for that payload but fail-open for the already validated
host and Administrator bridge.

The runtime registry isolates installer exceptions, aggregates sanitized health,
disposes the prior renderer generation before reload, and is itself disposed by
Administrator. The native manager is catalog-driven; adding another reviewed
project should require a manifest entry and adapter review, not a hardcoded UI
branch.

`enabled` in the preparation report means reviewed assets were admitted, not
that the renderer installer is active. The manager uses runtime health for the
active/pending/failed distinction and requires the runtime revision to match the
admitted revision. A cleanup failure keeps the original lifecycle object,
seals that registry against all new applies, and prevents bootstrap deletion
until cleanup leaves the namespaced state key absent. Lifecycle and global
accessor reads stay inside the failure boundary. An initially unreadable global
may be recaptured only through the same getter-only accessor identity and only
as an object or function; primitive values, foreign data-property replacements,
and accessors with setters are rejected. Once a blocking pending installer
seals the registry, later deferred installers remain pending and are never
mixed with unresolved residue. Cleanup success is determined by final
namespaced-global absence, including a lifecycle that deletes its state and
then throws.

Retained 2026-07-16 visual evidence proves real pointer activation, singleton
manager/dialog/addon rendering, matching runtime ID/revision health across a
renderer reload, explicit disposal, daily-process preservation, and exact
Direct-owned cleanup. A separate three-port Direct run proved no cross-instance
addon state and ordered independent shutdown. These results prove Direct-only
composition and isolation, not Codex++ eligibility.

Only one component may own a model ID. When Administrator owns `grok-*`,
Codex++ `modelWhitelistUnlock` must not publish overlapping `grok-*` entries.
Do not route an unmarked collision merely because its visible model ID matches;
surface the conflict and keep it unroutable instead of hijacking another
provider's catalog entry.

Official ChatGPT, Codex++, and each optional injector retain independent update
ownership. Re-evaluate exact host and payload identities on every launch.
Unknown updates never trigger patching, source edits, daily-instance fallback,
or cross-project cleanup; they disable only the incompatible adapter until a
new reviewed compatibility entry and E2E proof exist.

## Use Next Time

The composition registry, generic manager, Direct + Dream reload gate,
three-port Direct isolation gate, and configured-provider request gate now
exist. The next parity slices are files/images, structured output, cancellation,
automated restart/resume, and multi-port model execution. Codex++ +
Administrator + Dream remains blocked until a future Codex++ build first proves
isolated profile, `CODEX_HOME`, SQLite, owned PID tree, zero daily-state writes,
and daily-PID preservation. Unknown official/Codex++/addon versions must remain
independent fail-closed gates before coexistence is claimed.
