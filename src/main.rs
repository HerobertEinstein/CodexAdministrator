use std::{
    collections::BTreeSet,
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read, Write},
    net::TcpListener,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use codex_administrator::{
    BootstrapConfig, CompatibilityDecision, CompatibilityManifest, CompatibilityPolicy,
    DEFAULT_GROK_ACTION_PATH, DEFAULT_GROK_BASE_URL, DirectInstance, DirectInstanceLayout,
    DiscoveredModel, GrokControlBroker, GrokNativeProviderConfig, HostAdapterKind, HostIdentity,
    InjectedModelDescriptor, LauncherSettings, ModelPickerConfig, NativeSessionChangeMonitor,
    NativeSessionContinuityCoordinator, NativeSessionContinuityProcessBackend, NativeSessionLane,
    PROVIDER_CREDENTIAL_TARGET, RendererAddonPolicy, RendererAddonReport, RendererAddonSettings,
    SupervisorMode, WindowsCredentialStore, WindowsDirectRuntime, codex_plus_launch_allowed,
    fetch_model_list, find_official_chatgpt_executable, install_grok_native_provider,
    launch_host_executable, launcher_instance_root, launcher_settings_path,
    native_session_continuity_hook_response, native_shared_session_rollouts,
    prepare_codex_plus_host_script_guarded, prepare_renderer_addons,
    recent_native_shared_thread_ids, remove_codex_plus_bootstrap, render_bootstrap,
    resolve_launcher_control_settings, run_native_session_continuity_worker,
    sync_native_session_continuity_hooks_via_official_app_server,
    validate_launchable_official_chatgpt_executable, validate_official_chatgpt_executable,
};
use directories::BaseDirs;
use serde::Serialize;
use windows_sys::Win32::{
    Foundation::ERROR_SUCCESS,
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6432KEY,
        RRF_SUBKEY_WOW6464KEY, RegGetValueW,
    },
};

#[derive(Debug, Parser)]
#[command(
    name = "codex-administrator",
    version,
    about = "External Grok model-list injection for native ChatGPT/Codex hosts"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    ConfigureProvider(ConfigureProviderArgs),
    Inject(Box<InjectArgs>),
    Doctor(DoctorArgs),
    #[command(hide = true)]
    SessionContinuityWorker(SessionContinuityWorkerArgs),
    #[command(hide = true)]
    SessionContinuityHook(SessionContinuityHookArgs),
}

#[derive(Debug, Args)]
struct ConfigureProviderArgs {
    #[arg(long, value_name = "URL")]
    base_url: String,

    #[arg(long, value_name = "PATH", default_value = DEFAULT_GROK_ACTION_PATH)]
    action_path: String,

    #[arg(long, value_name = "ENV", default_value = "XAI_API_KEY")]
    env_key: String,

    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct InjectArgs {
    #[arg(long, value_enum, default_value_t = HostAdapterKind::Direct)]
    host: HostAdapterKind,

    #[arg(long = "model", value_name = "MODEL")]
    models: Vec<String>,

    #[arg(long = "renderer-addon", value_name = "ID=ABSOLUTE_PATH", hide = true)]
    renderer_addons: Vec<String>,

    #[arg(long, value_name = "URL")]
    base_url: Option<String>,

    #[arg(long, value_name = "PATH", default_value = DEFAULT_GROK_ACTION_PATH)]
    action_path: String,

    #[arg(long, hide = true)]
    manual_action_path: bool,

    #[arg(long, value_name = "ENV", default_value = "XAI_API_KEY")]
    env_key: String,

    #[arg(long, value_name = "EXE")]
    official_path: Option<PathBuf>,

    #[arg(long, value_name = "DIR", hide = true)]
    instance_root: Option<PathBuf>,

    #[arg(long, hide = true)]
    retain_instance_root: bool,

    #[arg(long, hide = true)]
    sync_native_auth: bool,

    #[arg(long, hide = true)]
    sync_native_sessions: bool,

    #[arg(long, hide = true)]
    sync_native_goals: bool,

    #[arg(long, hide = true)]
    sync_native_skills: bool,

    #[arg(long, hide = true)]
    credential_present: bool,

    #[arg(long, value_name = "DIR", hide = true)]
    daily_profile: Option<PathBuf>,

    #[arg(long, value_name = "DIR", hide = true)]
    daily_codex_home: Option<PathBuf>,

    #[arg(long, value_name = "PORT", hide = true)]
    cdp_port: Option<u16>,

    #[arg(long, value_name = "SECONDS", default_value_t = 90, hide = true)]
    startup_timeout_seconds: u64,

    #[arg(long, value_name = "SECONDS", hide = true)]
    session_timeout_seconds: Option<u64>,

    #[arg(long, value_name = "EXE")]
    codex_plus_path: Option<PathBuf>,

    #[arg(long, value_name = "DIR", hide = true)]
    appdata: Option<PathBuf>,

    #[arg(long)]
    no_launch: bool,

    #[arg(long, hide = true)]
    launcher_managed: bool,
}

#[derive(Debug, Args)]
struct DoctorArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct SessionContinuityWorkerArgs {
    #[arg(long)]
    daily_codex_home: PathBuf,

    #[arg(long)]
    isolated_codex_home: PathBuf,

    #[arg(long)]
    request: PathBuf,

    #[arg(long)]
    manifest: PathBuf,

    #[arg(long, hide = true)]
    parent_gated: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SessionContinuityLaneArg {
    Daily,
    Isolated,
}

impl From<SessionContinuityLaneArg> for NativeSessionLane {
    fn from(value: SessionContinuityLaneArg) -> Self {
        match value {
            SessionContinuityLaneArg::Daily => Self::Daily,
            SessionContinuityLaneArg::Isolated => Self::Isolated,
        }
    }
}

#[derive(Debug, Args)]
struct SessionContinuityHookArgs {
    #[arg(long, value_enum, hide = true)]
    lane: Option<SessionContinuityLaneArg>,

    #[arg(long, hide = true)]
    manifest: Option<PathBuf>,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::ConfigureProvider(args) => configure_provider(args),
        Command::Inject(args) => inject(*args),
        Command::Doctor(args) => doctor(args),
        Command::SessionContinuityWorker(args) => session_continuity_worker(args),
        Command::SessionContinuityHook(args) => session_continuity_hook(args),
    }
}

fn session_continuity_worker(args: SessionContinuityWorkerArgs) -> Result<()> {
    if args.parent_gated {
        let mut gate = [0_u8; 1];
        io::stdin()
            .read_exact(&mut gate)
            .context("continuity worker parent gate closed before release")?;
    }
    let receipt = run_native_session_continuity_worker(
        &args.daily_codex_home,
        &args.isolated_codex_home,
        &args.request,
        &args.manifest,
    )?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "status": "session_continuity_observed",
            "threads": receipt.threads,
            "equal": receipt.equal,
            "daily_ahead": receipt.daily_ahead,
            "isolated_ahead": receipt.isolated_ahead,
            "diverged": receipt.diverged,
            "unknown": receipt.unknown,
        }))?
    );
    Ok(())
}

fn session_continuity_hook(args: SessionContinuityHookArgs) -> Result<()> {
    const MAX_HOOK_INPUT_BYTES: u64 = 64 * 1024;
    let isolated_codex_home = launcher_instance_root()?.join("codex-home");
    let manifest = args
        .manifest
        .unwrap_or_else(|| isolated_codex_home.join("session-continuity-manifest.json"));
    let lane = match args.lane {
        Some(lane) => lane.into(),
        None => {
            let current = env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .and_then(|path| fs::canonicalize(path).ok());
            let isolated = fs::canonicalize(&isolated_codex_home).ok();
            if current.is_some() && current == isolated {
                NativeSessionLane::Isolated
            } else {
                NativeSessionLane::Daily
            }
        }
    };
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_HOOK_INPUT_BYTES + 1)
        .read_to_end(&mut input)?;
    if input.len() as u64 > MAX_HOOK_INPUT_BYTES {
        bail!("session continuity hook input exceeds its size limit");
    }
    println!(
        "{}",
        serde_json::to_string(&native_session_continuity_hook_response(
            &manifest, lane, &input,
        )?)?
    );
    Ok(())
}

fn configure_provider(args: ConfigureProviderArgs) -> Result<()> {
    let provider = GrokNativeProviderConfig {
        base_url: args.base_url,
        action_path: args.action_path,
        env_key: args.env_key,
        supports_websockets: false,
    };
    provider.validate()?;
    if env::var_os(&provider.env_key).is_none_or(|value| value.is_empty()) {
        bail!(
            "required provider credential environment variable {} is not set",
            provider.env_key
        );
    }

    let config = active_codex_config(args.config)?;
    let receipt = install_grok_native_provider(&config, &provider)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "status": "provider_configured",
            "provider": codex_administrator::GROK_NATIVE_PROVIDER_ID,
            "config": config,
            "config_updated": receipt.updated,
            "config_sha256": receipt.sha256,
        }))?
    );
    Ok(())
}

fn inject(args: InjectArgs) -> Result<()> {
    let renderer_addons = parse_renderer_addon_settings(&args.renderer_addons)?;
    let addon_policy = RendererAddonPolicy::shipped()?;
    let addon_bundle = prepare_renderer_addons(&renderer_addons, &addon_policy, args.host);
    let addon_catalog = addon_policy.catalog(args.host);
    let addon_reports = addon_bundle.reports().to_vec();
    let model_picker = ModelPickerConfig {
        host_adapter: args.host,
        base_url: args
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_GROK_BASE_URL.into()),
        action_path: args.action_path.clone(),
        action_path_auto: !args.manual_action_path,
        sync_native_auth: args.sync_native_auth,
        sync_native_sessions: args.sync_native_sessions,
        sync_native_goals: args.sync_native_goals,
        sync_native_skills: args.sync_native_skills,
        credential_present: args.credential_present
            || env::var_os(&args.env_key).is_some_and(|value| !value.is_empty()),
        renderer_addons: renderer_addons.clone(),
        renderer_addon_catalog: addon_catalog,
        renderer_addon_reports: addon_reports.clone(),
        control_nonce: ModelPickerConfig::default().control_nonce,
    };
    let bootstrap_config = BootstrapConfig {
        models: args
            .models
            .iter()
            .cloned()
            .map(InjectedModelDescriptor::grok)
            .collect(),
        model_picker,
    };
    let core_bootstrap = render_bootstrap(&bootstrap_config)?;
    let bootstrap = addon_bundle.compose(&core_bootstrap);

    match args.host {
        HostAdapterKind::Direct => inject_direct(
            args,
            bootstrap_config,
            bootstrap,
            renderer_addons,
            addon_reports,
        ),
        HostAdapterKind::CodexPlusPlus => inject_codex_plus(args, bootstrap, addon_reports),
    }
}

fn inject_direct(
    args: InjectArgs,
    bootstrap_config: BootstrapConfig,
    bootstrap: String,
    renderer_addons: Vec<RendererAddonSettings>,
    renderer_addon_reports: Vec<RendererAddonReport>,
) -> Result<()> {
    let provider = if args.models.is_empty() {
        None
    } else {
        let provider = GrokNativeProviderConfig {
            base_url: args
                .base_url
                .clone()
                .ok_or_else(|| anyhow::anyhow!("direct injection requires --base-url"))?,
            action_path: args.action_path.clone(),
            env_key: args.env_key.clone(),
            supports_websockets: false,
        };
        provider.validate()?;
        if env::var_os(&provider.env_key).is_none_or(|value| value.is_empty()) {
            bail!(
                "required provider credential environment variable {} is not set",
                provider.env_key
            );
        }
        Some(provider)
    };
    let provider_id = provider
        .as_ref()
        .map(|_| codex_administrator::GROK_NATIVE_PROVIDER_ID);
    let mode = if provider.is_some() {
        SupervisorMode::Configured
    } else {
        SupervisorMode::ManagementOnly
    };

    let executable = match args.official_path.clone() {
        Some(path) => path,
        None => find_official_chatgpt_executable()?,
    };
    validate_official_chatgpt_executable(&executable)?;
    validate_launchable_official_chatgpt_executable(&executable)?;
    let base_dirs = BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("unable to resolve Windows user directories"))?;
    let daily_profile = args.daily_profile.clone().unwrap_or_else(|| {
        base_dirs
            .config_dir()
            .join("Codex")
            .join("web")
            .join("Codex")
    });
    let daily_codex_home = args
        .daily_codex_home
        .clone()
        .unwrap_or_else(|| base_dirs.home_dir().join(".codex"));
    let root = args
        .instance_root
        .clone()
        .unwrap_or(default_direct_instance_root()?);
    let cdp_port = match args.cdp_port {
        Some(port) => {
            verify_loopback_port_available(port)?;
            port
        }
        None => allocate_loopback_port()?,
    };
    let layout = DirectInstanceLayout::new(
        root.clone(),
        executable.clone(),
        daily_profile,
        daily_codex_home,
        cdp_port,
    )?;
    if args.no_launch {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "status": "validated",
                "host": args.host,
                "injection_enabled": false,
                "reason": "no_launch",
                "mode": mode,
                "official_executable": executable,
                "instance_root": root,
                "cdp_port": cdp_port,
                "provider": provider_id,
                "models": args.models,
                "renderer_addons": renderer_addon_reports,
            }))?
        );
        return Ok(());
    }

    if (args.sync_native_auth
        || args.sync_native_sessions
        || args.sync_native_goals
        || args.sync_native_skills)
        && !args.retain_instance_root
    {
        bail!("native state synchronization requires --retain-instance-root");
    }
    let control_nonce = bootstrap_config.model_picker.control_nonce.clone();
    let runtime_settings = LauncherSettings {
        base_url: bootstrap_config.model_picker.base_url.clone(),
        action_path: bootstrap_config.model_picker.action_path.clone(),
        action_path_auto: bootstrap_config.model_picker.action_path_auto,
        selected_models: args.models.clone(),
        cached_models: args
            .models
            .iter()
            .cloned()
            .map(|id| DiscoveredModel { id, owned_by: None })
            .collect(),
        renderer_addons,
        sync_native_auth: args.sync_native_auth,
        sync_native_sessions: args.sync_native_sessions,
        sync_native_goals: args.sync_native_goals,
        sync_native_skills: args.sync_native_skills,
        ..LauncherSettings::default()
    };
    let settings_path = launcher_settings_path()?;
    let initial_settings =
        resolve_launcher_control_settings(&settings_path, runtime_settings, args.launcher_managed)?;
    let credential_present = args.credential_present
        || env::var_os(&args.env_key).is_some_and(|value| !value.is_empty());
    let credential_store = WindowsCredentialStore::new(PROVIDER_CREDENTIAL_TARGET);
    let mut control_broker = GrokControlBroker::new(
        control_nonce.clone(),
        initial_settings,
        credential_present,
        settings_path,
    )?;
    let injected_models = bootstrap_config.models;
    let runtime = if args.sync_native_auth
        || args.sync_native_sessions
        || args.sync_native_goals
        || args.sync_native_skills
    {
        WindowsDirectRuntime::new_retained_with_native_state_sync_and_injected_models(
            root.clone(),
            provider,
            injected_models,
            args.sync_native_auth,
            args.sync_native_sessions,
            args.sync_native_goals,
            args.sync_native_skills,
        )?
    } else if args.retain_instance_root {
        WindowsDirectRuntime::new_retained_with_injected_models(
            root.clone(),
            provider,
            injected_models,
        )?
    } else {
        WindowsDirectRuntime::new_with_injected_models(root.clone(), provider, injected_models)?
    };
    let runtime = if mode == SupervisorMode::ManagementOnly {
        runtime.with_blocked_environment_key(&args.env_key)?
    } else {
        runtime
    };
    let timeout = Duration::from_secs(args.startup_timeout_seconds);
    let stopping = Arc::new(AtomicBool::new(false));
    let stopping_for_handler = Arc::clone(&stopping);
    ctrlc::set_handler(move || stopping_for_handler.store(true, Ordering::SeqCst))
        .context("failed to install direct-instance shutdown handler")?;
    let mut instance =
        DirectInstance::start(layout.contract().clone(), bootstrap, runtime, timeout)?;
    if stopping.load(Ordering::SeqCst) {
        instance.shutdown()?;
        bail!("direct startup was interrupted");
    }
    let mut session_continuity = if args.sync_native_sessions {
        let daily_codex_home = layout.contract().daily_codex_home().to_path_buf();
        let isolated_codex_home = layout.contract().isolated_codex_home().to_path_buf();
        let continuity_executable =
            env::current_exe().context("failed to resolve continuity helper executable")?;
        let continuity_manifest = isolated_codex_home.join("session-continuity-manifest.json");
        let executable_text = continuity_executable.to_string_lossy();
        let manifest_text = continuity_manifest.to_string_lossy();
        if executable_text.contains('"')
            || manifest_text.contains('"')
            || executable_text.chars().any(char::is_control)
            || manifest_text.chars().any(char::is_control)
        {
            bail!("session continuity helper paths cannot be represented safely as a hook command");
        }
        let hook_command =
            format!("\"{executable_text}\" session-continuity-hook --manifest \"{manifest_text}\"");
        sync_native_session_continuity_hooks_via_official_app_server(
            &daily_codex_home,
            &isolated_codex_home,
            &hook_command,
        )?
        .ok_or_else(|| {
            anyhow::anyhow!("official Codex app-server is unavailable for continuity hook trust")
        })?;
        let rollouts = native_shared_session_rollouts(&daily_codex_home, &isolated_codex_home)?;
        if rollouts.is_empty() {
            None
        } else {
            let seed_threads = recent_native_shared_thread_ids(&rollouts, 8)?;
            let monitor = NativeSessionChangeMonitor::new(
                &daily_codex_home,
                &isolated_codex_home,
                rollouts,
                Duration::from_millis(750),
            )?;
            let backend = NativeSessionContinuityProcessBackend::new(
                continuity_executable,
                daily_codex_home,
                isolated_codex_home.clone(),
                isolated_codex_home.join("session-continuity-worker-request.json"),
                continuity_manifest,
            )?;
            let mut coordinator =
                NativeSessionContinuityCoordinator::new(monitor, Box::new(backend));
            coordinator.enqueue_threads(seed_threads)?;
            coordinator.maintain_once()?;
            Some(coordinator)
        }
    } else {
        None
    };
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "status": "ready",
            "host": args.host,
            "injection_enabled": true,
            "mode": mode,
            "official_executable": executable,
            "instance_root": root,
            "cdp_port": cdp_port,
            "target": instance.target(),
            "provider": provider_id,
            "models": args.models,
            "renderer_addons": renderer_addon_reports,
        }))?
    );
    io::stdout().flush()?;

    let started = Instant::now();
    let mut restart_requested = false;
    while !stopping.load(Ordering::SeqCst)
        && args
            .session_timeout_seconds
            .is_none_or(|seconds| started.elapsed() < Duration::from_secs(seconds))
    {
        thread::sleep(Duration::from_millis(500));
        if instance.maintain_once()? == codex_administrator::DirectMaintenance::Exited {
            break;
        }
        let disable_session_continuity =
            if let Some(session_continuity) = session_continuity.as_mut() {
                match session_continuity.maintain_once() {
                    Ok(receipt) => {
                        if let Some(finished) = receipt.finished
                            && !finished.outcome.success
                        {
                            eprintln!(
                                "session continuity worker failed: {}",
                                finished.outcome.diagnostic
                            );
                        }
                        false
                    }
                    Err(error) => {
                        eprintln!("session continuity monitor disabled: {error}");
                        true
                    }
                }
            } else {
                false
            };
        if disable_session_continuity {
            session_continuity = None;
        }
        for request in instance.drain_control_requests(&control_nonce)? {
            let outcome = control_broker.handle(request, &credential_store, fetch_model_list);
            let request_restart = outcome.restart_required;
            instance.deliver_control_response(outcome.response)?;
            if request_restart {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "status": "restart_requested",
                        "host": args.host,
                        "instance_root": root,
                    }))?
                );
                io::stdout().flush()?;
                restart_requested = true;
                break;
            }
        }
        if restart_requested {
            break;
        }
    }
    drop(session_continuity.take());
    instance.shutdown()?;
    Ok(())
}

fn inject_codex_plus(
    args: InjectArgs,
    bootstrap: String,
    renderer_addon_reports: Vec<RendererAddonReport>,
) -> Result<()> {
    let appdata = args.appdata.unwrap_or(default_appdata()?);
    let executable = args
        .codex_plus_path
        .or_else(|| find_codex_plus_plus().path)
        .ok_or_else(|| anyhow::anyhow!("unable to locate the installed Codex++ executable"))?;

    let (policy, manifest_error) =
        match CompatibilityManifest::shipped().and_then(CompatibilityManifest::into_policy) {
            Ok(policy) => (policy, None),
            Err(error) => (
                CompatibilityPolicy::default(),
                Some(format!(
                    "shipped compatibility manifest is invalid: {error}"
                )),
            ),
        };
    let (identity, identity_error) =
        match HostIdentity::from_executable(HostAdapterKind::CodexPlusPlus, &executable) {
            Ok(identity) => (Some(identity), None),
            Err(error) => (None, Some(format!("host identity probe failed: {error}"))),
        };

    let mut outcome =
        prepare_codex_plus_host_script_guarded(&appdata, &bootstrap, identity.as_ref(), &policy);
    let mut warnings = Vec::new();
    if outcome.bootstrap.is_some()
        && !identity
            .as_ref()
            .is_some_and(|identity| identity.matches_executable(&executable).unwrap_or(false))
    {
        if let Err(error) = remove_codex_plus_bootstrap(&appdata) {
            warnings.push(format!(
                "bootstrap cleanup after identity change failed: {error}"
            ));
        }
        warnings.push("host identity changed after compatibility evaluation".into());
        outcome = codex_administrator::CodexPlusStartupOutcome {
            decision: CompatibilityDecision::NativeOnly {
                reason: "host_identity_changed_before_launch".into(),
            },
            bootstrap: None,
            isolation_error: None,
        };
    }

    let launch_allowed = codex_plus_launch_allowed(args.no_launch, &outcome);
    if !args.no_launch && !launch_allowed {
        warnings.push("unverified Codex++ host was not launched".into());
    }
    if launch_allowed && let Err(error) = launch_host_executable(&executable) {
        let cleanup = remove_codex_plus_bootstrap(&appdata)
            .err()
            .map(|cleanup| format!("; bootstrap cleanup also failed: {cleanup}"))
            .unwrap_or_default();
        bail!("failed to launch Codex++: {error}{cleanup}");
    }

    let reason = match &outcome.decision {
        CompatibilityDecision::Enabled => None,
        CompatibilityDecision::NativeOnly { reason } => Some(reason.as_str()),
    };
    warnings.extend(
        [
            manifest_error,
            identity_error,
            outcome.isolation_error.clone(),
        ]
        .into_iter()
        .flatten(),
    );
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "status": if outcome.bootstrap.is_some() { "ready" } else { "native_fallback" },
            "host": args.host,
            "injection_enabled": outcome.decision.injection_enabled(),
            "reason": reason,
            "host_identity": identity,
            "bootstrap": outcome.bootstrap.as_ref().map(|receipt| serde_json::json!({
                "path": receipt.bootstrap_path,
                "sha256": receipt.sha256,
            })),
            "warnings": warnings,
            "renderer_addons": renderer_addon_reports,
        }))?
    );
    Ok(())
}

fn parse_renderer_addon_settings(values: &[String]) -> Result<Vec<RendererAddonSettings>> {
    if values.len() > 16 {
        bail!("too many renderer addons were requested");
    }
    let mut seen = BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let (id, source_root) = value
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("renderer addon must use ID=ABSOLUTE_PATH"))?;
            let setting = RendererAddonSettings {
                id: id.to_owned(),
                enabled: true,
                source_root: PathBuf::from(source_root),
            };
            setting.validate()?;
            if !seen.insert(setting.id.clone()) {
                bail!("renderer addon IDs must be unique");
            }
            Ok(setting)
        })
        .collect()
}

fn doctor(args: DoctorArgs) -> Result<()> {
    let direct_probe = find_official_chatgpt_executable();
    let (direct_available, direct_reason) = match &direct_probe {
        Ok(_) => (true, "ready".to_string()),
        Err(error) => (false, error.to_string()),
    };
    let report = DoctorReport {
        product: "Codex Administrator",
        version: env!("CARGO_PKG_VERSION"),
        platform: env::consts::OS,
        adapters: AdapterReport {
            direct: DirectAdapterReport {
                implemented: true,
                available: direct_available,
                reason: direct_reason,
            },
            codex_plus_plus: codex_plus_plus_doctor_report(),
        },
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Codex Administrator {}", report.version);
        println!(
            "Direct: {} ({})",
            if report.adapters.direct.available {
                "available"
            } else {
                "unavailable"
            },
            report.adapters.direct.reason
        );
        println!(
            "Codex++: {}",
            display_codex_plus_adapter(&report.adapters.codex_plus_plus)
        );
    }
    Ok(())
}

fn default_appdata() -> Result<PathBuf> {
    BaseDirs::new()
        .map(|dirs| dirs.config_dir().to_path_buf())
        .ok_or_else(|| anyhow::anyhow!("unable to resolve the configuration directory"))
}

fn default_codex_config() -> Result<PathBuf> {
    if let Some(codex_home) = env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(codex_home).join("config.toml"));
    }
    BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".codex").join("config.toml"))
        .ok_or_else(|| anyhow::anyhow!("unable to resolve the Codex config path"))
}

fn active_codex_config(requested: Option<PathBuf>) -> Result<PathBuf> {
    let default_config = default_codex_config()?;
    let config = requested.unwrap_or_else(|| default_config.clone());
    if config != default_config {
        bail!(
            "official ChatGPT/Codex reads {}; refusing unrelated config path {}",
            default_config.display(),
            config.display()
        );
    }
    Ok(config)
}

fn find_codex_plus_plus() -> ProbeResult {
    let env_path = env::var_os("CODEX_PLUS_PLUS_PATH").map(PathBuf::from);
    let program_files = env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .map(|path| path.join("Codex++").join("codex-plus-plus.exe"));
    let local_program = BaseDirs::new().map(|dirs| {
        dirs.data_local_dir()
            .join("Programs")
            .join("Codex++")
            .join("codex-plus-plus.exe")
    });
    let registered = registered_codex_plus_plus_install_locations()
        .into_iter()
        .map(|path| path.join("codex-plus-plus.exe"));
    ProbeResult::from_path(
        env_path
            .into_iter()
            .chain(program_files)
            .chain(local_program)
            .chain(registered)
            .find(|candidate| candidate.is_file()),
    )
}

fn codex_plus_plus_doctor_report() -> CodexPlusAdapterReport {
    let probe = find_codex_plus_plus();
    let Some(path) = probe.path else {
        return CodexPlusAdapterReport {
            found: false,
            path: None,
            eligible: false,
            reason: "not_found".into(),
        };
    };
    let identity = match HostIdentity::from_executable(HostAdapterKind::CodexPlusPlus, &path) {
        Ok(identity) => identity,
        Err(_) => {
            return CodexPlusAdapterReport {
                found: true,
                path: Some(path),
                eligible: false,
                reason: "host_identity_probe_failed".into(),
            };
        }
    };
    let policy = match CompatibilityManifest::shipped().and_then(CompatibilityManifest::into_policy)
    {
        Ok(policy) => policy,
        Err(_) => {
            return CodexPlusAdapterReport {
                found: true,
                path: Some(path),
                eligible: false,
                reason: "invalid_compatibility_manifest".into(),
            };
        }
    };
    match policy.evaluate(HostAdapterKind::CodexPlusPlus, Some(&identity.sha256)) {
        CompatibilityDecision::Enabled => CodexPlusAdapterReport {
            found: true,
            path: Some(path),
            eligible: true,
            reason: "ready".into(),
        },
        CompatibilityDecision::NativeOnly { reason } => CodexPlusAdapterReport {
            found: true,
            path: Some(path),
            eligible: false,
            reason,
        },
    }
}

fn registered_codex_plus_plus_install_locations() -> Vec<PathBuf> {
    const UNINSTALL_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexPlusPlus";
    [
        (HKEY_CURRENT_USER, RRF_RT_REG_SZ),
        (HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY),
        (HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ | RRF_SUBKEY_WOW6432KEY),
    ]
    .into_iter()
    .filter_map(|(root, flags)| registry_string(root, UNINSTALL_KEY, "InstallLocation", flags))
    .map(PathBuf::from)
    .collect()
}

fn registry_string(root: HKEY, subkey: &str, value_name: &str, flags: u32) -> Option<OsString> {
    const MAX_REGISTRY_STRING_BYTES: u32 = 32 * 1024;
    let subkey = wide_null(subkey);
    let value_name = wide_null(value_name);
    let mut value_type = 0;
    let mut bytes = 0;
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value_name.as_ptr(),
            flags,
            &mut value_type,
            ptr::null_mut(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS
        || value_type != REG_SZ
        || !(2..=MAX_REGISTRY_STRING_BYTES).contains(&bytes)
        || bytes % 2 != 0
    {
        return None;
    }
    let mut buffer = vec![0_u16; bytes as usize / 2];
    let status = unsafe {
        RegGetValueW(
            root,
            subkey.as_ptr(),
            value_name.as_ptr(),
            flags,
            &mut value_type,
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS || value_type != REG_SZ || bytes % 2 != 0 {
        return None;
    }
    let units = bytes as usize / 2;
    buffer.truncate(units.min(buffer.len()));
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    (!buffer.is_empty()).then(|| OsString::from_wide(&buffer))
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    product: &'static str,
    version: &'static str,
    platform: &'static str,
    adapters: AdapterReport,
}

#[derive(Debug, Serialize)]
struct AdapterReport {
    direct: DirectAdapterReport,
    codex_plus_plus: CodexPlusAdapterReport,
}

#[derive(Debug, Serialize)]
struct DirectAdapterReport {
    implemented: bool,
    available: bool,
    reason: String,
}

#[derive(Debug, Serialize)]
struct CodexPlusAdapterReport {
    found: bool,
    path: Option<PathBuf>,
    eligible: bool,
    reason: String,
}

fn default_direct_instance_root() -> Result<PathBuf> {
    let base_dirs = BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("unable to resolve the local data directory"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(base_dirs
        .data_local_dir()
        .join("CodexAdministrator")
        .join("instances")
        .join(format!("{:x}-{timestamp:x}", std::process::id())))
}

fn allocate_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .context("failed to allocate an isolated loopback CDP port")?;
    Ok(listener.local_addr()?.port())
}

fn verify_loopback_port_available(port: u16) -> Result<()> {
    if port < 1024 {
        bail!("isolated CDP port must not use a system-reserved port");
    }
    TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("isolated loopback CDP port {port} is unavailable"))?;
    Ok(())
}

#[derive(Debug, Serialize)]
struct ProbeResult {
    found: bool,
    path: Option<PathBuf>,
}

impl ProbeResult {
    fn from_path(path: Option<PathBuf>) -> Self {
        Self {
            found: path.is_some(),
            path,
        }
    }
}

fn display_codex_plus_adapter(report: &CodexPlusAdapterReport) -> String {
    let Some(path) = report.path.as_deref() else {
        return format!("not found ({})", report.reason);
    };
    if report.eligible {
        format!("{} (eligible)", path.display())
    } else {
        format!("{} (native only: {})", path.display(), report.reason)
    }
}
