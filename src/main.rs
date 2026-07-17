use std::{
    collections::BTreeSet,
    env,
    io::{self, Write},
    net::TcpListener,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use codex_administrator::{
    BootstrapConfig, CompatibilityDecision, CompatibilityManifest, CompatibilityPolicy,
    DEFAULT_GROK_ACTION_PATH, DEFAULT_GROK_BASE_URL, DirectInstance, DirectInstanceLayout,
    DiscoveredModel, GrokControlBroker, GrokNativeProviderConfig, HostAdapterKind, HostIdentity,
    InjectedModelDescriptor, LauncherSettings, ModelPickerConfig, PROVIDER_CREDENTIAL_TARGET,
    RendererAddonPolicy, RendererAddonReport, RendererAddonSettings, SupervisorMode,
    WindowsCredentialStore, WindowsDirectRuntime, codex_plus_launch_allowed, fetch_model_list,
    find_official_chatgpt_executable, install_grok_native_provider, launch_host_executable,
    launcher_settings_path, prepare_codex_plus_host_script_guarded, prepare_renderer_addons,
    remove_codex_plus_bootstrap, render_bootstrap, resolve_launcher_control_settings,
    validate_launchable_official_chatgpt_executable, validate_official_chatgpt_executable,
};
use directories::BaseDirs;
use serde::Serialize;

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

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::ConfigureProvider(args) => configure_provider(args),
        Command::Inject(args) => inject(*args),
        Command::Doctor(args) => doctor(args),
    }
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

    if (args.sync_native_auth || args.sync_native_sessions) && !args.retain_instance_root {
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
    let runtime = if args.sync_native_auth || args.sync_native_sessions {
        WindowsDirectRuntime::new_retained_with_native_state_sync_and_injected_models(
            root.clone(),
            provider,
            injected_models,
            args.sync_native_auth,
            args.sync_native_sessions,
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
            codex_plus_plus: find_codex_plus_plus(),
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
            display_probe(&report.adapters.codex_plus_plus)
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
    ProbeResult::from_path(
        env_path
            .into_iter()
            .chain(program_files)
            .chain(local_program)
            .find(|candidate| candidate.is_file()),
    )
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
    codex_plus_plus: ProbeResult,
}

#[derive(Debug, Serialize)]
struct DirectAdapterReport {
    implemented: bool,
    available: bool,
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

fn display_probe(probe: &ProbeResult) -> String {
    probe
        .path
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not found".into())
}
