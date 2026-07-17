use std::fmt;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::LauncherSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorMode {
    ManagementOnly,
    Configured,
}

pub struct SupervisorGeneration {
    mode: SupervisorMode,
    settings: LauncherSettings,
    launch_settings: LauncherSettings,
    credential_present: bool,
    credential: Option<Zeroizing<String>>,
}

impl SupervisorGeneration {
    pub fn new(settings: LauncherSettings, credential: Option<String>) -> Result<Self> {
        settings.validate()?;
        let mut credential = credential.map(Zeroizing::new);
        if let Some(credential) = &credential {
            validate_credential(credential)?;
        }
        let credential_present = credential.is_some();
        let mode = if settings.selected_models.is_empty() || credential.is_none() {
            SupervisorMode::ManagementOnly
        } else {
            SupervisorMode::Configured
        };
        let mut launch_settings = settings.clone();
        if mode == SupervisorMode::ManagementOnly {
            launch_settings.selected_models.clear();
            launch_settings.sync_native_sessions = false;
            credential = None;
        }
        Ok(Self {
            mode,
            settings,
            launch_settings,
            credential_present,
            credential,
        })
    }

    pub const fn mode(&self) -> SupervisorMode {
        self.mode
    }

    pub const fn settings(&self) -> &LauncherSettings {
        &self.settings
    }

    pub const fn launch_settings(&self) -> &LauncherSettings {
        &self.launch_settings
    }

    pub fn credential(&self) -> Option<&str> {
        self.credential.as_deref().map(String::as_str)
    }

    pub const fn credential_present(&self) -> bool {
        self.credential_present
    }
}

impl fmt::Debug for SupervisorGeneration {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SupervisorGeneration")
            .field("mode", &self.mode)
            .field("settings", &self.settings)
            .field("launch_settings", &self.launch_settings)
            .field("credential_present", &self.credential_present)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherChildEvent {
    Ready { mode: SupervisorMode },
    RestartRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherChildOutcome {
    pub ready_mode: Option<SupervisorMode>,
    pub restart_requested: bool,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub diagnostic: String,
}

pub trait LauncherSupervisorBackend {
    fn load_generation(&mut self) -> Result<SupervisorGeneration>;

    fn run_generation(&mut self, generation: &SupervisorGeneration)
    -> Result<LauncherChildOutcome>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorExit {
    UserClosed,
}

pub fn supervise_launcher<B: LauncherSupervisorBackend>(
    backend: &mut B,
    max_restarts: usize,
) -> Result<SupervisorExit> {
    let mut restart_count = 0_usize;
    loop {
        let generation = backend.load_generation()?;
        let outcome = backend.run_generation(&generation)?;
        if outcome.ready_mode != Some(generation.mode()) {
            return bail_with_outcome(
                "isolated child did not report readiness for the requested mode",
                &outcome,
            );
        }
        if outcome.restart_requested {
            if !outcome.success {
                bail_with_outcome("isolated child failed while requesting restart", &outcome)?;
            }
            if restart_count >= max_restarts {
                bail!("isolated child exceeded the bounded restart limit");
            }
            restart_count += 1;
            continue;
        }
        if outcome.success {
            return Ok(SupervisorExit::UserClosed);
        }
        bail_with_outcome("isolated child exited unexpectedly", &outcome)?;
    }
}

fn bail_with_outcome(prefix: &str, outcome: &LauncherChildOutcome) -> Result<SupervisorExit> {
    let detail = outcome
        .diagnostic
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .map(str::to_owned)
        .or_else(|| outcome.exit_code.map(|code| format!("exit code {code}")))
        .unwrap_or_else(|| "no diagnostic was returned".into());
    bail!("{prefix}: {detail}")
}

pub fn parse_launcher_child_event(line: &str) -> Option<LauncherChildEvent> {
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    if value.get("host").and_then(serde_json::Value::as_str) != Some("direct") {
        return None;
    }
    match value.get("status").and_then(serde_json::Value::as_str)? {
        "ready"
            if value
                .get("injection_enabled")
                .and_then(serde_json::Value::as_bool)
                == Some(true) =>
        {
            let mode = serde_json::from_value(value.get("mode")?.clone()).ok()?;
            Some(LauncherChildEvent::Ready { mode })
        }
        "restart_requested" => Some(LauncherChildEvent::RestartRequested),
        _ => None,
    }
}

fn validate_credential(credential: &str) -> Result<()> {
    if credential.is_empty()
        || credential.len() > 2048
        || credential.trim() != credential
        || credential.chars().any(char::is_control)
    {
        bail!("provider API key is invalid");
    }
    Ok(())
}
