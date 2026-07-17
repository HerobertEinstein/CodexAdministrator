use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    DiscoveredModel, RendererAddonSettings, install_bootstrap_atomically,
    is_reviewed_grok_model_id, model_list_url,
};

const SETTINGS_VERSION: u8 = 1;
const MAX_SELECTED_MODELS: usize = 128;
const MAX_CACHED_MODELS: usize = 4096;
const MAX_RENDERER_ADDONS: usize = 16;

pub const DEFAULT_GROK_BASE_URL: &str = "https://ai.hebox.net/v1";
pub const DEFAULT_GROK_ACTION_PATH: &str = "/responses";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LauncherSettings {
    pub version: u8,
    pub base_url: String,
    pub action_path: String,
    pub action_path_auto: bool,
    pub selected_models: Vec<String>,
    pub cached_models: Vec<DiscoveredModel>,
    pub renderer_addons: Vec<RendererAddonSettings>,
    pub sync_native_auth: bool,
    pub sync_native_sessions: bool,
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            base_url: DEFAULT_GROK_BASE_URL.into(),
            action_path: DEFAULT_GROK_ACTION_PATH.into(),
            action_path_auto: true,
            selected_models: Vec::new(),
            cached_models: Vec::new(),
            renderer_addons: Vec::new(),
            sync_native_auth: true,
            sync_native_sessions: true,
        }
    }
}

impl LauncherSettings {
    pub fn validate(&self) -> Result<()> {
        if self.version != SETTINGS_VERSION {
            bail!("launcher settings version is unsupported");
        }
        if self.cached_models.len() > MAX_CACHED_MODELS
            || self.selected_models.len() > MAX_SELECTED_MODELS
            || self.renderer_addons.len() > MAX_RENDERER_ADDONS
        {
            bail!("launcher settings exceed a bounded collection limit");
        }
        let mut addons = BTreeSet::new();
        for addon in &self.renderer_addons {
            addon.validate()?;
            if !addons.insert(addon.id.as_str()) {
                bail!("launcher settings contain a duplicate renderer addon");
            }
        }
        if self.base_url.is_empty() {
            if !self.selected_models.is_empty() || !self.cached_models.is_empty() {
                bail!("launcher settings require a provider base URL");
            }
            return Ok(());
        }
        model_list_url(&self.base_url)?;
        provider_base_url_for_action_path(&self.base_url, &self.action_path)?;
        let mut cached = BTreeSet::new();
        for model in &self.cached_models {
            if !is_reviewed_grok_model_id(&model.id) {
                bail!("launcher settings contain a model without a reviewed capability profile");
            }
            if !cached.insert(model.id.as_str()) {
                bail!("launcher settings contain a duplicate cached model");
            }
        }
        let mut selected = BTreeSet::new();
        for model in &self.selected_models {
            if !selected.insert(model.as_str()) {
                bail!("launcher settings contain a duplicate selected model");
            }
            if !model.to_ascii_lowercase().starts_with("grok-") {
                bail!("version 1 can select only Grok models");
            }
            if !cached.contains(model.as_str()) {
                bail!("launcher settings select a model absent from the cached catalog");
            }
        }
        Ok(())
    }
}

pub fn provider_base_url_for_action_path(base_url: &str, action_path: &str) -> Result<String> {
    model_list_url(base_url)?;
    validate_responses_action_path(action_path)?;
    let prefix = action_path
        .strip_suffix(DEFAULT_GROK_ACTION_PATH)
        .ok_or_else(|| anyhow::anyhow!("action path must end in /responses"))?;
    Ok(format!("{}{}", base_url.trim_end_matches('/'), prefix))
}

fn validate_responses_action_path(action_path: &str) -> Result<()> {
    if action_path.is_empty()
        || action_path.len() > 256
        || action_path.trim() != action_path
        || !action_path.starts_with('/')
        || !action_path.ends_with(DEFAULT_GROK_ACTION_PATH)
        || action_path.contains(['\\', '?', '#'])
        || action_path.contains("//")
        || action_path.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '/' | '-' | '_' | '.' | '~')
        })
        || action_path
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
    {
        bail!("action path must be a relative Responses-compatible path");
    }
    Ok(())
}

pub fn load_launcher_settings(path: &Path) -> Result<LauncherSettings> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LauncherSettings::default());
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read launcher settings {}", path.display()));
        }
    };
    if bytes.len() > 4 * 1024 * 1024 {
        bail!("launcher settings file is too large");
    }
    let settings: LauncherSettings = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse launcher settings {}", path.display()))?;
    settings.validate()?;
    Ok(settings)
}

pub fn save_launcher_settings(path: &Path, settings: &LauncherSettings) -> Result<()> {
    settings.validate()?;
    let content = serde_json::to_vec_pretty(settings)?;
    install_bootstrap_atomically(path, &content)?;
    Ok(())
}

pub fn resolve_launcher_control_settings(
    path: &Path,
    fallback: LauncherSettings,
    launcher_managed: bool,
) -> Result<LauncherSettings> {
    if launcher_managed {
        load_launcher_settings(path)
    } else {
        fallback.validate()?;
        Ok(fallback)
    }
}
