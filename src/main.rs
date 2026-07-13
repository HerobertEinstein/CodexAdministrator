use std::{
    env,
    path::PathBuf,
    process::{Command as ProcessCommand, Stdio},
};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use codex_administrator::{
    BootstrapConfig, CodexNativeAppLaunchSpec, CompanionContext, CompatibilityDecision,
    CompatibilityManifest, CompatibilityPolicy, GrokNativeProviderConfig, HostAdapterKind,
    HostIdentity, build_codex_native_app_launch, build_companion_router, discover_codex_runtime,
    generate_capability, install_grok_native_provider_for_model, launch_host_executable,
    prepare_codex_plus_host_guarded, remove_codex_plus_bootstrap, restore_native_model_selection,
    validate_codex_model_catalog_with_runtime,
};
use directories::BaseDirs;
use serde::Serialize;
use tokio::net::TcpListener;

#[derive(Debug, Parser)]
#[command(
    name = "codex-administrator",
    version,
    about = "Open-source Windows launcher for native ChatGPT/Codex model providers"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Launch(LaunchArgs),
    LaunchNative(LaunchNativeArgs),
    Serve(ServeArgs),
    Doctor(DoctorArgs),
}

#[derive(Debug, Args)]
struct LaunchArgs {
    #[arg(long, value_name = "MODEL")]
    model: String,

    #[arg(long, value_name = "URL")]
    base_url: String,

    #[arg(long, value_name = "ENV", default_value = "XAI_API_KEY")]
    env_key: String,

    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[arg(long, value_name = "FILE")]
    model_catalog: Option<PathBuf>,

    #[arg(long, value_name = "DIR", default_value = ".")]
    workspace: PathBuf,
}

#[derive(Debug, Args)]
struct LaunchNativeArgs {
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[arg(long, value_name = "DIR", default_value = ".")]
    workspace: PathBuf,
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[arg(long, value_enum, default_value_t = HostAdapterKind::Direct)]
    host: HostAdapterKind,

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

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Launch(args) => launch_grok_native(args),
        Command::LaunchNative(args) => launch_native(args),
        Command::Serve(args) => serve(args).await,
        Command::Doctor(args) => doctor(args),
    }
}

fn launch_grok_native(args: LaunchArgs) -> Result<()> {
    let provider = GrokNativeProviderConfig {
        base_url: args.base_url,
        env_key: args.env_key,
        supports_websockets: false,
    };
    provider.validate()?;
    let secret_present = env::var_os(&provider.env_key).is_some_and(|value| !value.is_empty());
    if !secret_present {
        bail!(
            "required provider credential environment variable {} is not set",
            provider.env_key
        );
    }

    let workspace = std::fs::canonicalize(&args.workspace).with_context(|| {
        format!(
            "failed to resolve Codex workspace {}",
            args.workspace.display()
        )
    })?;
    if !workspace.is_dir() {
        bail!(
            "Codex workspace is not a directory: {}",
            workspace.display()
        );
    }
    let runtime = discover_codex_runtime()
        .ok_or_else(|| anyhow::anyhow!("unable to locate the official Codex runtime"))?;
    let model_catalog = match args.model_catalog {
        Some(path) => {
            let path = std::fs::canonicalize(&path).with_context(|| {
                format!("failed to resolve Codex model catalog {}", path.display())
            })?;
            validate_codex_model_catalog_with_runtime(&runtime, &path, &args.model)?;
            Some(path)
        }
        None => None,
    };
    let launch = build_codex_native_app_launch(&runtime, &workspace)?;
    let config = active_codex_config(args.config)?;
    let receipt = install_grok_native_provider_for_model(
        &config,
        &provider,
        &args.model,
        model_catalog.as_deref(),
    )?;

    run_codex_app(&launch, &workspace)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "status": "launched",
            "host": "official_codex_app",
            "provider": codex_administrator::GROK_NATIVE_PROVIDER_ID,
            "model": args.model,
            "model_catalog": model_catalog,
            "workspace": workspace,
            "config": config,
            "config_updated": receipt.updated,
            "config_sha256": receipt.sha256,
            "launcher_exit": "success",
        }))?
    );
    Ok(())
}

fn launch_native(args: LaunchNativeArgs) -> Result<()> {
    let workspace = std::fs::canonicalize(&args.workspace).with_context(|| {
        format!(
            "failed to resolve Codex workspace {}",
            args.workspace.display()
        )
    })?;
    if !workspace.is_dir() {
        bail!(
            "Codex workspace is not a directory: {}",
            workspace.display()
        );
    }
    let runtime = discover_codex_runtime()
        .ok_or_else(|| anyhow::anyhow!("unable to locate the official Codex runtime"))?;
    let launch = build_codex_native_app_launch(&runtime, &workspace)?;
    let config = active_codex_config(args.config)?;
    let receipt = restore_native_model_selection(&config)?;
    run_codex_app(&launch, &workspace)?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "status": "launched",
            "host": "official_codex_app",
            "provider": "restored_native_selection",
            "workspace": workspace,
            "config": config,
            "config_updated": receipt.updated,
            "config_sha256": receipt.sha256,
            "launcher_exit": "success",
        }))?
    );
    Ok(())
}

fn run_codex_app(launch: &CodexNativeAppLaunchSpec, workspace: &std::path::Path) -> Result<()> {
    let mut command = ProcessCommand::new(&launch.executable);
    command
        .args(&launch.args)
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to launch official Codex app via {}",
            launch.executable.display()
        )
    })?;
    if !status.success() {
        bail!("official Codex app launcher exited with {status}");
    }
    Ok(())
}

async fn serve(args: ServeArgs) -> Result<()> {
    match args.host {
        HostAdapterKind::CodexPlusPlus => serve_codex_plus(args).await,
        HostAdapterKind::Direct => serve_direct(args).await,
    }
}

async fn serve_direct(args: ServeArgs) -> Result<()> {
    if !args.no_launch {
        bail!(
            "the direct host launcher is not available in this alpha build; use --no-launch for companion UI development"
        );
    }
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .context("failed to bind companion loopback listener")?;
    let address = listener.local_addr()?;
    let capability = generate_capability();
    let context = CompanionContext::new(&capability)?;
    let ready = serde_json::json!({
        "status": "ready",
        "host": args.host,
        "address": address,
        "bootstrap": null,
    });
    println!("{}", serde_json::to_string(&ready)?);

    axum::serve(listener, build_companion_router(context))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("companion server failed")
}

async fn serve_codex_plus(args: ServeArgs) -> Result<()> {
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

    let listener = match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await {
        Ok(listener) => listener,
        Err(error) => {
            let cleanup_error = remove_codex_plus_bootstrap(&appdata).err();
            if !args.no_launch {
                launch_host_executable(&executable)?;
            }
            let cleanup = cleanup_error
                .map(|cleanup| format!("; stale bootstrap cleanup also failed: {cleanup}"))
                .unwrap_or_default();
            bail!("companion unavailable; official Codex++ was left native: {error}{cleanup}");
        }
    };
    let address = listener.local_addr()?;
    let capability = generate_capability();
    let context = CompanionContext::new(&capability)?;
    let bootstrap_config = BootstrapConfig {
        port: address.port(),
        capability,
    };
    let mut outcome =
        prepare_codex_plus_host_guarded(&appdata, &bootstrap_config, identity.as_ref(), &policy);
    let mut startup_warnings = Vec::new();
    if outcome.bootstrap.is_some()
        && !identity
            .as_ref()
            .is_some_and(|identity| identity.matches_executable(&executable).unwrap_or(false))
    {
        let cleanup_error = remove_codex_plus_bootstrap(&appdata).err();
        startup_warnings.push(
            "host identity changed or became unreadable after compatibility evaluation".into(),
        );
        if let Some(cleanup_error) = cleanup_error {
            startup_warnings.push(format!(
                "bootstrap cleanup after identity change failed: {cleanup_error}"
            ));
        }
        outcome = codex_administrator::CodexPlusStartupOutcome {
            decision: CompatibilityDecision::NativeOnly {
                requested: codex_administrator::AgentMode::GrokNativeModel,
                reason: "host_identity_changed_before_launch".into(),
            },
            bootstrap: None,
            isolation_error: None,
        };
    }

    if !args.no_launch
        && let Err(error) = launch_host_executable(&executable)
    {
        let cleanup_error = remove_codex_plus_bootstrap(&appdata).err();
        let cleanup = cleanup_error
            .map(|cleanup| format!("; bootstrap cleanup also failed: {cleanup}"))
            .unwrap_or_default();
        bail!("failed to launch Codex++: {error}{cleanup}");
    }

    let reason = match &outcome.decision {
        CompatibilityDecision::Enabled(_) => None,
        CompatibilityDecision::NativeOnly { reason, .. } => Some(reason.as_str()),
    };
    startup_warnings.extend(
        [
            manifest_error,
            identity_error,
            outcome.isolation_error.clone(),
        ]
        .into_iter()
        .flatten(),
    );
    let ready = serde_json::json!({
        "status": if outcome.bootstrap.is_some() { "ready" } else { "native_fallback" },
        "host": args.host,
        "address": address,
        "effective_mode": outcome.decision.effective_mode(),
        "injection_enabled": outcome.decision.injection_enabled(),
        "reason": reason,
        "host_identity": identity,
        "bootstrap": outcome.bootstrap.as_ref().map(|receipt| serde_json::json!({
            "path": receipt.bootstrap_path,
            "sha256": receipt.sha256,
        })),
        "warnings": startup_warnings,
    });
    println!("{}", serde_json::to_string(&ready)?);

    if outcome.bootstrap.is_none() {
        return Ok(());
    }
    axum::serve(listener, build_companion_router(context))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("companion server failed")
}

fn doctor(args: DoctorArgs) -> Result<()> {
    let report = DoctorReport {
        product: "Codex Administrator",
        version: env!("CARGO_PKG_VERSION"),
        platform: env::consts::OS,
        runtimes: RuntimeReport {
            codex: ProbeResult::from_runtime(discover_codex_runtime()),
        },
        hosts: HostReport {
            codex_plus_plus: find_codex_plus_plus(),
        },
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Codex Administrator {}", report.version);
        println!("Codex: {}", display_probe(&report.runtimes.codex));
        println!("Codex++: {}", display_probe(&report.hosts.codex_plus_plus));
    }
    Ok(())
}

fn default_appdata() -> Result<PathBuf> {
    BaseDirs::new()
        .map(|dirs| dirs.config_dir().to_path_buf())
        .ok_or_else(|| {
            anyhow::anyhow!("unable to resolve the current user's configuration directory")
        })
}

fn default_codex_config() -> Result<PathBuf> {
    if let Some(codex_home) = env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(codex_home).join("config.toml"));
    }
    BaseDirs::new()
        .map(|dirs| dirs.home_dir().join(".codex").join("config.toml"))
        .ok_or_else(|| anyhow::anyhow!("unable to resolve the current user's Codex config path"))
}

fn active_codex_config(requested: Option<PathBuf>) -> Result<PathBuf> {
    let default_config = default_codex_config()?;
    let config = requested.unwrap_or_else(|| default_config.clone());
    if config != default_config {
        bail!(
            "official Codex Desktop reads {}; refusing unrelated config path {}",
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

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    product: &'static str,
    version: &'static str,
    platform: &'static str,
    runtimes: RuntimeReport,
    hosts: HostReport,
}

#[derive(Debug, Serialize)]
struct RuntimeReport {
    codex: ProbeResult,
}

#[derive(Debug, Serialize)]
struct HostReport {
    codex_plus_plus: ProbeResult,
}

#[derive(Debug, Serialize)]
struct ProbeResult {
    found: bool,
    path: Option<PathBuf>,
    args: Vec<String>,
}

impl ProbeResult {
    fn from_path(path: Option<PathBuf>) -> Self {
        Self {
            found: path.is_some(),
            path,
            args: Vec::new(),
        }
    }

    fn from_runtime(runtime: Option<codex_administrator::RuntimeLaunchSpec>) -> Self {
        match runtime {
            Some(runtime) => Self {
                found: true,
                path: Some(runtime.executable),
                args: runtime.args,
            },
            None => Self {
                found: false,
                path: None,
                args: Vec::new(),
            },
        }
    }
}

fn display_probe(probe: &ProbeResult) -> String {
    probe
        .path
        .as_deref()
        .map(|path| {
            let mut display = path.display().to_string();
            if !probe.args.is_empty() {
                display.push(' ');
                display.push_str(&probe.args.join(" "));
            }
            display
        })
        .unwrap_or_else(|| "not found".into())
}
