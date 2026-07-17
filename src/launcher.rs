use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use anyhow::{Context, Result, bail};
use directories::BaseDirs;

use crate::{LauncherChildEvent, LauncherSettings, parse_launcher_child_event};

pub const PROVIDER_RUNTIME_ENV_KEY: &str = "CODEX_ADMINISTRATOR_PROVIDER_API_KEY";
const MAX_LAUNCHER_DIAGNOSTIC_INPUT_BYTES: usize = 64 * 1024;
const MAX_LAUNCHER_DIAGNOSTIC_DISPLAY_BYTES: usize = 4096;

pub fn launcher_settings_path() -> Result<PathBuf> {
    Ok(local_product_root()?.join("launcher-settings.json"))
}

pub fn launcher_instance_root() -> Result<PathBuf> {
    Ok(local_product_root()?.join("instances").join("default"))
}

pub fn build_direct_launcher_arguments(
    settings: &LauncherSettings,
    instance_root: &Path,
    credential_present: bool,
) -> Result<Vec<OsString>> {
    settings.validate()?;
    if !instance_root.is_absolute() {
        bail!("launcher instance root must be absolute");
    }

    let mut arguments = vec![
        OsString::from("inject"),
        OsString::from("--host"),
        OsString::from("direct"),
        OsString::from("--launcher-managed"),
    ];
    for model in &settings.selected_models {
        arguments.push(OsString::from("--model"));
        arguments.push(OsString::from(model));
    }
    for addon in settings
        .renderer_addons
        .iter()
        .filter(|addon| addon.enabled)
    {
        let mut value = OsString::from(&addon.id);
        value.push("=");
        value.push(&addon.source_root);
        arguments.push(OsString::from("--renderer-addon"));
        arguments.push(value);
    }
    arguments.extend([
        OsString::from("--base-url"),
        OsString::from(&settings.base_url),
        OsString::from("--action-path"),
        OsString::from(&settings.action_path),
        OsString::from("--env-key"),
        OsString::from(PROVIDER_RUNTIME_ENV_KEY),
        OsString::from("--instance-root"),
        instance_root.as_os_str().to_owned(),
        OsString::from("--retain-instance-root"),
    ]);
    if !settings.action_path_auto {
        arguments.push(OsString::from("--manual-action-path"));
    }
    if settings.sync_native_auth {
        arguments.push(OsString::from("--sync-native-auth"));
    }
    if settings.sync_native_sessions {
        arguments.push(OsString::from("--sync-native-sessions"));
    }
    if settings.sync_native_goals {
        arguments.push(OsString::from("--sync-native-goals"));
    }
    if settings.sync_native_skills {
        arguments.push(OsString::from("--sync-native-skills"));
    }
    if credential_present {
        arguments.push(OsString::from("--credential-present"));
    }
    Ok(arguments)
}

pub fn spawn_direct_launcher(
    executable: &Path,
    settings: &LauncherSettings,
    instance_root: &Path,
    api_key: Option<&str>,
    credential_present: bool,
) -> Result<Child> {
    if let Some(api_key) = api_key {
        validate_api_key(api_key)?;
    }
    if api_key.is_some() && !credential_present {
        bail!("provider credential presence is inconsistent");
    }
    let arguments = build_direct_launcher_arguments(settings, instance_root, credential_present)?;
    let mut command = Command::new(executable);
    command.args(arguments);
    for (key, _) in std::env::vars_os() {
        if environment_variable_is_sensitive(&key) {
            command.env_remove(key);
        }
    }
    command
        .env_remove(PROVIDER_RUNTIME_ENV_KEY)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(api_key) = api_key {
        command.env(PROVIDER_RUNTIME_ENV_KEY, api_key);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .with_context(|| format!("failed to launch {}", executable.display()))
}

pub fn environment_variable_is_sensitive(key: &OsStr) -> bool {
    let key = key.to_string_lossy().to_ascii_uppercase();
    key == "OPENAI_API_KEY"
        || key.ends_with("_API_KEY")
        || key.ends_with("_TOKEN")
        || key.ends_with("_SECRET")
        || key.ends_with("_PASSWORD")
        || key.ends_with("_PAT")
        || key.ends_with("_PWD")
        || key.ends_with("_CONNECTION_STRING")
        || matches!(
            key.as_str(),
            "API_KEY"
                | "AWS_ACCESS_KEY_ID"
                | "AWS_SECRET_ACCESS_KEY"
                | "CONNECTION_STRING"
                | "DATABASE_URL"
                | "GOOGLE_APPLICATION_CREDENTIALS"
                | "PASSWORD"
                | "PAT"
                | "PGPASSWORD"
                | "SECRET"
                | "TOKEN"
        )
}

pub fn launcher_output_is_ready(line: &str) -> bool {
    matches!(
        parse_launcher_child_event(line),
        Some(LauncherChildEvent::Ready { .. })
    )
}

pub fn sanitize_launcher_diagnostic(bytes: &[u8], secret: &str) -> String {
    let start = bytes
        .len()
        .saturating_sub(MAX_LAUNCHER_DIAGNOSTIC_INPUT_BYTES);
    let mut text = String::from_utf8_lossy(&bytes[start..]).into_owned();
    if !secret.is_empty() {
        text = text.replace(secret, "[REDACTED]");
    }
    let mut sanitized = text
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if let Some(index) = lower.find("authorization: bearer") {
                format!("{}Authorization: [REDACTED]", &line[..index])
            } else {
                line.chars()
                    .map(|character| {
                        if character.is_control() && character != '\t' {
                            ' '
                        } else {
                            character
                        }
                    })
                    .collect()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    if sanitized.len() > MAX_LAUNCHER_DIAGNOSTIC_DISPLAY_BYTES {
        let mut start = sanitized.len() - MAX_LAUNCHER_DIAGNOSTIC_DISPLAY_BYTES;
        while !sanitized.is_char_boundary(start) {
            start += 1;
        }
        sanitized = sanitized[start..].to_owned();
    }
    sanitized
}

fn local_product_root() -> Result<PathBuf> {
    BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("CodexAdministrator"))
        .ok_or_else(|| anyhow::anyhow!("unable to resolve the local application data directory"))
}

fn validate_api_key(api_key: &str) -> Result<()> {
    if api_key.is_empty()
        || api_key.len() > 2048
        || api_key.trim() != api_key
        || api_key.chars().any(char::is_control)
    {
        bail!("provider API key is invalid");
    }
    Ok(())
}
