use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Grok,
    Codex,
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
