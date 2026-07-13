//! Shared contracts for the Codex Administrator launcher and companion.

mod bootstrap;
mod companion;
mod compatibility;
mod host;
mod jsonl;
mod mode;
mod native_provider;
pub mod protocol;
mod runtime;
mod runtime_client;
mod runtime_process;
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
pub use jsonl::{JsonlEvent, JsonlTransport};
pub use mode::{AgentMode, ModeState};
pub use native_provider::{
    CodexNativeAppLaunchSpec, GROK_NATIVE_PROVIDER_ID, GrokNativeProviderConfig,
    NativeProviderCapabilities, NativeProviderCapabilityManifest, NativeProviderInstallReceipt,
    build_codex_native_app_launch, install_grok_native_provider,
    install_grok_native_provider_for_model, restore_native_model_selection,
    validate_codex_model_catalog, validate_codex_model_catalog_with_runtime,
};
pub use runtime::{
    RuntimeKind, RuntimeLaunchSpec, RuntimeProbe, RuntimeProbeStatus, RuntimeProtocol,
    discover_codex_runtime, discover_codex_runtime_in, probe_runtime_version,
};
pub use runtime_client::{CodexAppServerClient, CodexApprovalDecision};
pub use runtime_process::RuntimeProcess;
pub use startup::{
    CodexPlusPreparation, CodexPlusStartupOutcome, generate_capability, prepare_codex_plus_host,
    prepare_codex_plus_host_guarded,
};
