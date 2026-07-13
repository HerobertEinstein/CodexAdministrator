//! Shared contracts for the Codex Administrator launcher and companion.

mod bootstrap;
mod companion;
mod compatibility;
mod host;
mod mode;
mod runtime;
mod startup;

pub use bootstrap::{BootstrapConfig, render_bootstrap};
pub use companion::{CompanionContext, build_companion_router};
pub use compatibility::{
    CompatibilityDecision, CompatibilityManifest, CompatibilityPolicy, HostIdentity,
};
pub use host::{
    CODEX_PLUS_BOOTSTRAP_KEY, CodexPlusRemovalReceipt, HostAdapterKind, InjectionStrategy,
    codex_plus_bootstrap_path, enable_codex_plus_bootstrap, install_bootstrap_atomically,
    launch_host_executable, remove_codex_plus_bootstrap,
};
pub use mode::{AgentMode, ModeState};
pub use runtime::{
    RuntimeKind, RuntimeLaunchSpec, RuntimeProbe, RuntimeProbeStatus, RuntimeProtocol,
    probe_runtime_version,
};
pub use startup::{
    CodexPlusPreparation, CodexPlusStartupOutcome, generate_capability, prepare_codex_plus_host,
    prepare_codex_plus_host_guarded,
};
