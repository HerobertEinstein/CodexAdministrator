//! External provider configuration and model-list injection for ChatGPT/Codex hosts.

mod bootstrap;
mod cdp;
mod compatibility;
mod direct;
mod host;
mod isolation;
mod native_provider;
mod startup;
#[cfg(windows)]
mod windows_runtime;

pub use bootstrap::{
    BootstrapConfig, InjectedModelDescriptor, InjectedReasoningEffort, render_bootstrap,
};
pub use cdp::LoopbackCdpClient;
pub use compatibility::{
    CompatibilityDecision, CompatibilityManifest, CompatibilityPolicy, HostIdentity,
};
pub use direct::{
    DirectCdpTarget, DirectInstance, DirectInstanceLayout, DirectMaintenance, DirectRuntimeBackend,
};
pub use host::{
    CODEX_PLUS_BOOTSTRAP_KEY, CodexPlusRemovalReceipt, HostAdapterKind, InjectionStrategy,
    codex_plus_bootstrap_path, enable_codex_plus_bootstrap, install_bootstrap_atomically,
    launch_host_executable, remove_codex_plus_bootstrap,
};
pub use isolation::{DirectIsolationContract, IsolatedRuntimeObservation};
pub use native_provider::{
    GROK_NATIVE_PROVIDER_ID, GrokNativeProviderConfig, NativeProviderInstallReceipt,
    install_grok_native_provider,
};
pub use startup::{
    CodexPlusPreparation, CodexPlusStartupOutcome, prepare_codex_plus_host,
    prepare_codex_plus_host_guarded,
};
#[cfg(windows)]
pub use windows_runtime::{
    WindowsDirectRuntime, find_official_chatgpt_executable,
    validate_launchable_official_chatgpt_executable, validate_official_chatgpt_executable,
};
