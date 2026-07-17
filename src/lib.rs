//! External provider configuration and model-list injection for ChatGPT/Codex hosts.

mod bootstrap;
mod cdp;
mod compatibility;
mod control_protocol;
mod credential_store;
mod direct;
mod grok_control;
mod host;
mod isolation;
mod launcher;
mod launcher_settings;
mod model_discovery;
#[cfg(windows)]
mod native_goal_sync;
mod native_model_catalog;
mod native_provider;
#[cfg(windows)]
mod native_skill_sync;
#[cfg(windows)]
mod native_state_sync;
mod renderer_addons;
mod startup;
mod supervisor;
#[cfg(windows)]
mod windows_runtime;

pub use bootstrap::{
    BootstrapConfig, InjectedModelDescriptor, InjectedReasoningEffort, ModelPickerConfig,
    render_bootstrap,
};
pub use cdp::LoopbackCdpClient;
pub use compatibility::{
    CompatibilityDecision, CompatibilityManifest, CompatibilityPolicy, HostIdentity,
};
pub use control_protocol::{
    ControlOperation, ControlRequest, ControlResponse, parse_control_requests,
};
#[cfg(windows)]
pub use credential_store::WindowsCredentialStore;
pub use credential_store::{
    CredentialStore, PROVIDER_CREDENTIAL_TARGET, bind_provider_credential,
    resolve_bound_provider_credential,
};
pub use direct::{
    DirectCdpTarget, DirectInstance, DirectInstanceLayout, DirectMaintenance, DirectRuntimeBackend,
};
pub use grok_control::{GrokControlBroker, GrokControlOutcome};
pub use host::{
    CODEX_PLUS_BOOTSTRAP_KEY, CodexPlusRemovalReceipt, HostAdapterKind, InjectionStrategy,
    codex_plus_bootstrap_path, enable_codex_plus_bootstrap, install_bootstrap_atomically,
    launch_host_executable, remove_codex_plus_bootstrap,
};
pub use isolation::{DirectIsolationContract, IsolatedRuntimeObservation};
pub use launcher::{
    PROVIDER_RUNTIME_ENV_KEY, build_direct_launcher_arguments, environment_variable_is_sensitive,
    launcher_instance_root, launcher_output_is_ready, launcher_settings_path,
    sanitize_launcher_diagnostic, spawn_direct_launcher,
};
pub use launcher_settings::{
    DEFAULT_GROK_ACTION_PATH, DEFAULT_GROK_BASE_URL, LauncherSettings, load_launcher_settings,
    provider_base_url_for_action_path, resolve_launcher_control_settings, save_launcher_settings,
};
pub use model_discovery::{
    DiscoveredModel, fetch_model_list, injectable_grok_models, is_reviewed_grok_model_id,
    model_list_url, parse_model_list, search_models,
};
#[cfg(windows)]
pub use native_goal_sync::{
    NativeGoalIntent, NativeGoalStatus, NativeGoalStore, NativeGoalSyncReceipt,
    sync_native_goal_intents, sync_native_goal_intents_via_official_app_server,
};
pub use native_model_catalog::{
    install_grok_native_model_catalog, remove_grok_native_model_catalog,
};
pub use native_provider::{
    GROK_NATIVE_PROVIDER_ID, GrokNativeProviderConfig, NativeProviderInstallReceipt,
    install_grok_native_provider, remove_grok_native_provider,
};
#[cfg(windows)]
pub use native_skill_sync::{NativeSkillSyncReceipt, sync_native_skills};
#[cfg(windows)]
pub use native_state_sync::{
    NativeSessionSyncReceipt, install_isolated_sqlite_home, sync_native_session_snapshots,
};
pub use renderer_addons::{
    RendererAddonBundle, RendererAddonCatalogEntry, RendererAddonPolicy, RendererAddonReport,
    RendererAddonSettings, RendererAddonState, prepare_renderer_addons,
};
pub use startup::{
    CodexPlusPreparation, CodexPlusStartupOutcome, codex_plus_launch_allowed,
    prepare_codex_plus_host, prepare_codex_plus_host_guarded, prepare_codex_plus_host_script,
    prepare_codex_plus_host_script_guarded,
};
pub use supervisor::{
    LauncherChildEvent, LauncherChildOutcome, LauncherSupervisorBackend, SupervisorExit,
    SupervisorGeneration, SupervisorMode, parse_launcher_child_event, supervise_launcher,
};
#[cfg(windows)]
pub use windows_runtime::{
    WindowsDirectRuntime, find_installed_official_chatgpt_executable,
    find_official_chatgpt_executable, select_latest_official_package_candidate,
    validate_launchable_official_chatgpt_executable, validate_official_chatgpt_executable,
};
