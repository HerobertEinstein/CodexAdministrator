use std::{
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
    DirectInstance, DirectInstanceLayout, GrokNativeProviderConfig, HostAdapterKind, HostIdentity,
    InjectedModelDescriptor, WindowsDirectRuntime, find_official_chatgpt_executable,
    install_grok_native_provider, launch_host_executable, prepare_codex_plus_host_guarded,
    remove_codex_plus_bootstrap, render_bootstrap, validate_launchable_official_chatgpt_executable,
    validate_official_chatgpt_executable,
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

    #[arg(long, value_name = "ENV", default_value = "XAI_API_KEY")]
    env_key: String,

    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct InjectArgs {
    #[arg(long, value_enum, default_value_t = HostAdapterKind::Direct)]
    host: HostAdapterKind,

    #[arg(long = "model", value_name = "MODEL", required = true)]
    models: Vec<String>,

    #[arg(long, value_name = "URL")]
    base_url: Option<String>,

    #[arg(long, value_name = "ENV", default_value = "XAI_API_KEY")]
    env_key: String,

    #[arg(long, value_name = "EXE")]
    official_path: Option<PathBuf>,

    #[arg(long, value_name = "DIR", hide = true)]
    instance_root: Option<PathBuf>,

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
    let bootstrap_config = BootstrapConfig {
        models: args
            .models
            .iter()
            .cloned()
            .map(InjectedModelDescriptor::grok)
            .collect(),
    };
    render_bootstrap(&bootstrap_config)?;

    match args.host {
        HostAdapterKind::Direct => inject_direct(args, bootstrap_config),
        HostAdapterKind::CodexPlusPlus => inject_codex_plus(args, bootstrap_config),
    }
}

fn inject_direct(args: InjectArgs, bootstrap_config: BootstrapConfig) -> Result<()> {
    let provider = GrokNativeProviderConfig {
        base_url: args
            .base_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("direct injection requires --base-url"))?,
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
    let bootstrap = render_bootstrap(&bootstrap_config)?;

    if args.no_launch {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "status": "validated",
                "host": args.host,
                "injection_enabled": false,
                "reason": "no_launch",
                "official_executable": executable,
                "instance_root": root,
                "cdp_port": cdp_port,
                "provider": codex_administrator::GROK_NATIVE_PROVIDER_ID,
                "models": args.models,
            }))?
        );
        return Ok(());
    }

    let runtime = WindowsDirectRuntime::new(root.clone(), Some(provider))?;
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
            "official_executable": executable,
            "instance_root": root,
            "cdp_port": cdp_port,
            "target": instance.target(),
            "provider": codex_administrator::GROK_NATIVE_PROVIDER_ID,
            "models": args.models,
        }))?
    );
    io::stdout().flush()?;

    let started = Instant::now();
    while !stopping.load(Ordering::SeqCst)
        && args
            .session_timeout_seconds
            .is_none_or(|seconds| started.elapsed() < Duration::from_secs(seconds))
    {
        thread::sleep(Duration::from_millis(500));
        instance.maintain_once()?;
    }
    instance.shutdown()?;
    Ok(())
}

fn inject_codex_plus(args: InjectArgs, bootstrap_config: BootstrapConfig) -> Result<()> {
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
        prepare_codex_plus_host_guarded(&appdata, &bootstrap_config, identity.as_ref(), &policy);
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

    if !args.no_launch
        && let Err(error) = launch_host_executable(&executable)
    {
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
        }))?
    );
    Ok(())
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
