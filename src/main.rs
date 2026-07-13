use std::{env, path::PathBuf};

use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use codex_administrator::{
    BootstrapConfig, CompatibilityDecision, CompatibilityManifest, CompatibilityPolicy,
    GrokNativeProviderConfig, HostAdapterKind, HostIdentity, InjectedModelDescriptor,
    install_grok_native_provider, launch_host_executable, prepare_codex_plus_host_guarded,
    remove_codex_plus_bootstrap, render_bootstrap,
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
    Inject(InjectArgs),
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
        Command::Inject(args) => inject(args),
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
        HostAdapterKind::Direct => bail!(
            "direct ChatGPT/Codex injection is disabled pending desktop E2E; no official process or file was modified"
        ),
        HostAdapterKind::CodexPlusPlus => inject_codex_plus(args, bootstrap_config),
    }
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
    let report = DoctorReport {
        product: "Codex Administrator",
        version: env!("CARGO_PKG_VERSION"),
        platform: env::consts::OS,
        adapters: AdapterReport {
            direct: DirectAdapterReport {
                enabled: false,
                reason: "pending_desktop_e2e",
            },
            codex_plus_plus: find_codex_plus_plus(),
        },
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Codex Administrator {}", report.version);
        println!("Direct: disabled ({})", report.adapters.direct.reason);
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
    enabled: bool,
    reason: &'static str,
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
