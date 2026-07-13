use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use codex_administrator::{
    BootstrapConfig, CompanionContext, HostAdapterKind, build_companion_router,
    generate_capability, launch_host_executable, prepare_codex_plus_host,
};
use directories::BaseDirs;
use serde::Serialize;
use tokio::net::TcpListener;

#[derive(Debug, Parser)]
#[command(
    name = "codex-administrator",
    version,
    about = "Open-source Windows dual-main-agent launcher and injected GUI companion"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve(ServeArgs),
    Doctor(DoctorArgs),
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
        Command::Serve(args) => serve(args).await,
        Command::Doctor(args) => doctor(args),
    }
}

async fn serve(args: ServeArgs) -> Result<()> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .context("failed to bind companion loopback listener")?;
    let address = listener.local_addr()?;
    let capability = generate_capability();
    let context = CompanionContext::new(&capability)?;
    let bootstrap_config = BootstrapConfig {
        port: address.port(),
        capability,
    };

    let preparation = match args.host {
        HostAdapterKind::CodexPlusPlus => {
            let appdata = args.appdata.unwrap_or(default_appdata()?);
            let receipt = prepare_codex_plus_host(&appdata, &bootstrap_config)?;
            if !args.no_launch {
                let executable = args.codex_plus_path.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "--codex-plus-path is required when launching the Codex++ adapter"
                    )
                })?;
                launch_host_executable(executable)?;
            }
            Some(receipt)
        }
        HostAdapterKind::Direct => {
            if !args.no_launch {
                bail!(
                    "the direct host launcher is not available in this alpha build; use --no-launch for companion UI development"
                );
            }
            None
        }
    };

    let ready = serde_json::json!({
        "status": "ready",
        "host": args.host,
        "address": address,
        "bootstrap": preparation.as_ref().map(|receipt| serde_json::json!({
            "path": receipt.bootstrap_path,
            "sha256": receipt.sha256,
        })),
    });
    println!("{}", serde_json::to_string(&ready)?);

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
            grok: find_executable("grok.exe"),
            codex: find_executable("codex.exe"),
        },
        hosts: HostReport {
            codex_plus_plus: find_codex_plus_plus(),
        },
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Codex Administrator {}", report.version);
        println!("Grok: {}", display_probe(&report.runtimes.grok));
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

fn find_executable(name: &str) -> ProbeResult {
    let path = env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file());
    ProbeResult::from_path(path)
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
    grok: ProbeResult,
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
