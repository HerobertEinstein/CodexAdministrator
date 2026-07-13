use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Result, bail};
use serde::Serialize;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Grok,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProbeStatus {
    Available,
    Missing,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeProbe {
    pub kind: RuntimeKind,
    pub status: RuntimeProbeStatus,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeProtocol {
    AcpV1JsonLines,
    CodexAppServerJsonLines,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLaunchSpec {
    pub kind: RuntimeKind,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub protocol: RuntimeProtocol,
    pub use_shell: bool,
}

impl RuntimeLaunchSpec {
    pub fn grok(executable: PathBuf) -> Self {
        Self {
            kind: RuntimeKind::Grok,
            executable,
            args: ["agent", "--no-leader", "stdio"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            protocol: RuntimeProtocol::AcpV1JsonLines,
            use_shell: false,
        }
    }

    pub fn codex(executable: PathBuf) -> Self {
        Self {
            kind: RuntimeKind::Codex,
            executable,
            args: ["app-server", "--stdio"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            protocol: RuntimeProtocol::CodexAppServerJsonLines,
            use_shell: false,
        }
    }

    pub fn validate_executable_path(path: &Path) -> Result<()> {
        if !path.is_absolute() {
            bail!("runtime executable path must be absolute");
        }
        let is_executable = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"));
        if !is_executable {
            bail!("runtime executable must be a Windows .exe file");
        }
        Ok(())
    }
}

pub async fn probe_runtime_version(
    kind: RuntimeKind,
    executable: &Path,
    timeout: Duration,
) -> RuntimeProbe {
    if !executable.is_file() {
        return RuntimeProbe {
            kind,
            status: RuntimeProbeStatus::Missing,
            version: None,
            error: Some(format!(
                "runtime executable does not exist: {}",
                executable.display()
            )),
        };
    }

    let mut command = Command::new(executable);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return RuntimeProbe {
                kind,
                status: RuntimeProbeStatus::Failed,
                version: None,
                error: Some(format!("failed to start runtime version probe: {error}")),
            };
        }
    };

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => RuntimeProbe {
            kind,
            status: RuntimeProbeStatus::TimedOut,
            version: None,
            error: Some("runtime version probe timed out".into()),
        },
        Ok(Err(error)) => RuntimeProbe {
            kind,
            status: RuntimeProbeStatus::Failed,
            version: None,
            error: Some(format!("runtime version probe failed: {error}")),
        },
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            RuntimeProbe {
                kind,
                status: RuntimeProbeStatus::Available,
                version: Some(if stdout.is_empty() { stderr } else { stdout }),
                error: None,
            }
        }
        Ok(Ok(output)) => RuntimeProbe {
            kind,
            status: RuntimeProbeStatus::Failed,
            version: None,
            error: Some(format!(
                "runtime version probe exited with {}",
                output.status
            )),
        },
    }
}
