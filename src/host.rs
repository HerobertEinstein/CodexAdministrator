use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const CODEX_PLUS_BOOTSTRAP_KEY: &str = "user:codex-administrator-bootstrap.js";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexPlusRemovalReceipt {
    pub script_removed: bool,
    pub config_updated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum HostAdapterKind {
    Direct,
    #[value(name = "codexplusplus")]
    CodexPlusPlus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionStrategy {
    ProjectOwnedCdp,
    ExternalUserScript,
}

impl HostAdapterKind {
    pub const fn injection_strategy(self) -> InjectionStrategy {
        match self {
            Self::Direct => InjectionStrategy::ProjectOwnedCdp,
            Self::CodexPlusPlus => InjectionStrategy::ExternalUserScript,
        }
    }
}

pub fn codex_plus_bootstrap_path(appdata: &Path) -> PathBuf {
    appdata
        .join("Codex++")
        .join("user_scripts")
        .join("codex-administrator-bootstrap.js")
}

pub fn launch_host_executable(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("host executable does not exist: {}", path.display());
    }
    launch_host_executable_platform(path)
}

pub fn install_bootstrap_atomically(path: &Path, content: &[u8]) -> Result<String> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("bootstrap path has no parent"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create bootstrap directory {}", parent.display()))?;

    let temp_path = unique_temp_path(path);
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp_path)
            .with_context(|| format!("failed to create temporary file {}", temp_path.display()))?;
        file.write_all(content)
            .with_context(|| format!("failed to write temporary file {}", temp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temporary file {}", temp_path.display()))?;
        replace_file(&temp_path, path)
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result?;
    Ok(format!("{:x}", Sha256::digest(content)))
}

pub fn enable_codex_plus_bootstrap(config_path: &Path) -> Result<()> {
    let mut root = if config_path.exists() {
        let content = fs::read(config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        serde_json::from_slice::<Value>(&content)
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    } else {
        Value::Object(Map::new())
    };

    let object = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex++ user script config must be a JSON object"))?;
    object.insert("enabled".into(), Value::Bool(true));
    let scripts = object
        .entry("scripts")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex++ scripts config must be a JSON object"))?;
    scripts.insert(CODEX_PLUS_BOOTSTRAP_KEY.into(), Value::Bool(true));

    let content = serde_json::to_vec_pretty(&root)?;
    install_bootstrap_atomically(config_path, &content)?;
    Ok(())
}

pub fn remove_codex_plus_bootstrap(appdata: &Path) -> Result<CodexPlusRemovalReceipt> {
    let bootstrap_path = codex_plus_bootstrap_path(appdata);
    let script_removed = match fs::remove_file(&bootstrap_path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to remove bootstrap {}", bootstrap_path.display())
            });
        }
    };

    let config_path = appdata.join("Codex++").join("user_scripts.json");
    if !config_path.exists() {
        return Ok(CodexPlusRemovalReceipt {
            script_removed,
            config_updated: false,
        });
    }

    let content = fs::read(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let mut root: Value = serde_json::from_slice(&content)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    let object = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Codex++ user script config must be a JSON object"))?;
    let config_updated = object
        .get_mut("scripts")
        .and_then(Value::as_object_mut)
        .is_some_and(|scripts| scripts.remove(CODEX_PLUS_BOOTSTRAP_KEY).is_some());
    if config_updated {
        let content = serde_json::to_vec_pretty(&root)?;
        install_bootstrap_atomically(&config_path, &content)?;
    }

    Ok(CodexPlusRemovalReceipt {
        script_removed,
        config_updated,
    })
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let suffix: u64 = rand::rng().random();
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bootstrap");
    path.with_file_name(format!(".{name}.{suffix:016x}.tmp"))
}

#[cfg(windows)]
fn launch_host_executable_platform(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{UI::Shell::ShellExecuteW, UI::WindowsAndMessaging::SW_SHOWNORMAL};

    let operation = "runas\0".encode_utf16().collect::<Vec<_>>();
    let executable = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            executable.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        bail!(
            "Windows elevation launch failed with ShellExecute code {}",
            result as isize
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn launch_host_executable_platform(path: &Path) -> Result<()> {
    std::process::Command::new(path)
        .spawn()
        .with_context(|| format!("failed to launch {}", path.display()))?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        bail!(
            "failed to atomically replace bootstrap: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            target.display(),
            source.display()
        )
    })
}
