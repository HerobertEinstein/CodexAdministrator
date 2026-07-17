use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    DEFAULT_GROK_ACTION_PATH, DEFAULT_GROK_BASE_URL, GROK_NATIVE_PROVIDER_ID, HostAdapterKind,
    RendererAddonCatalogEntry, RendererAddonReport, RendererAddonSettings,
    is_reviewed_grok_model_id, provider_base_url_for_action_path,
};

const RENDERER_API_DISCOVERY: &str = include_str!("../assets/renderer-api-discovery.js");
const MODEL_INJECTION_CORE: &str = include_str!("../assets/model-injection-core.js");
const MODEL_PICKER_MOUNT: &str = include_str!("../assets/model-picker-mount.js");
const BOOTSTRAP_TEMPLATE: &str = include_str!("../assets/bootstrap.js");
static CONTROL_NONCE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectedReasoningEffort {
    pub reasoning_effort: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectedModelDescriptor {
    pub model: String,
    pub display_name: String,
    pub description: String,
    pub supported_reasoning_efforts: Vec<InjectedReasoningEffort>,
    pub default_reasoning_effort: String,
    pub input_modalities: Vec<String>,
}

impl InjectedModelDescriptor {
    pub fn grok(model: impl Into<String>) -> Self {
        let model = model.into();
        let (supported_reasoning_efforts, default_reasoning_effort) = if model == "grok-4.5" {
            (
                vec![
                    InjectedReasoningEffort {
                        reasoning_effort: "low".into(),
                        description: "Faster responses with lighter reasoning".into(),
                    },
                    InjectedReasoningEffort {
                        reasoning_effort: "medium".into(),
                        description: "Balanced reasoning".into(),
                    },
                    InjectedReasoningEffort {
                        reasoning_effort: "high".into(),
                        description: "Deeper reasoning (default)".into(),
                    },
                ],
                "high".into(),
            )
        } else if let Some(effort) = fixed_reasoning_effort(&model) {
            (
                vec![InjectedReasoningEffort {
                    reasoning_effort: effort.into(),
                    description: format!("Reasoning effort fixed by the {model} alias"),
                }],
                effort.into(),
            )
        } else {
            (
                vec![InjectedReasoningEffort {
                    reasoning_effort: "medium".into(),
                    description: "Balanced reasoning".into(),
                }],
                "medium".into(),
            )
        };
        let input_modalities = if model == "grok-4.5" {
            vec!["text".into(), "image".into()]
        } else {
            vec!["text".into()]
        };
        Self {
            display_name: model.clone(),
            description: "Configured xAI model; new tasks only".into(),
            model,
            supported_reasoning_efforts,
            default_reasoning_effort,
            input_modalities,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        validate_text(&self.model, "injected model id", 256, false)?;
        if !self.model.to_ascii_lowercase().starts_with("grok-") {
            bail!("Grok injection accepts only model IDs beginning with grok-");
        }
        if !is_reviewed_grok_model_id(&self.model) {
            bail!("Grok injection requires a reviewed capability profile for the exact model ID");
        }
        validate_text(
            &self.display_name,
            "injected model display name",
            128,
            false,
        )?;
        validate_text(&self.description, "injected model description", 512, true)?;
        if self.supported_reasoning_efforts.is_empty()
            || self.supported_reasoning_efforts.len() > 16
        {
            bail!("injected model must expose 1-16 declared reasoning efforts");
        }
        let mut efforts = BTreeSet::new();
        for effort in &self.supported_reasoning_efforts {
            validate_text(
                &effort.reasoning_effort,
                "injected model reasoning effort",
                32,
                false,
            )?;
            validate_text(
                &effort.description,
                "injected model reasoning description",
                256,
                false,
            )?;
            if !efforts.insert(effort.reasoning_effort.as_str()) {
                bail!("injected model contains a duplicate reasoning effort");
            }
        }
        if !efforts.contains(self.default_reasoning_effort.as_str()) {
            bail!("injected model default reasoning effort must be in its reviewed effort list");
        }
        if self.input_modalities.is_empty() || self.input_modalities.len() > 8 {
            bail!("injected model must expose 1-8 reviewed input modalities");
        }
        let mut modalities = BTreeSet::new();
        for modality in &self.input_modalities {
            if !matches!(modality.as_str(), "text" | "image") {
                bail!("injected model input modality must be text or image");
            }
            if !modalities.insert(modality.as_str()) {
                bail!("injected model contains a duplicate input modality");
            }
        }
        Ok(())
    }
}

fn fixed_reasoning_effort(model: &str) -> Option<&'static str> {
    let model = model.to_ascii_lowercase();
    ["xhigh", "high", "medium", "low"]
        .into_iter()
        .find(|effort| model.ends_with(&format!("-{effort}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapConfig {
    pub models: Vec<InjectedModelDescriptor>,
    pub model_picker: ModelPickerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPickerConfig {
    pub host_adapter: HostAdapterKind,
    pub base_url: String,
    pub action_path: String,
    pub action_path_auto: bool,
    pub sync_native_auth: bool,
    pub sync_native_sessions: bool,
    pub credential_present: bool,
    pub renderer_addons: Vec<RendererAddonSettings>,
    pub renderer_addon_catalog: Vec<RendererAddonCatalogEntry>,
    pub renderer_addon_reports: Vec<RendererAddonReport>,
    pub control_nonce: String,
}

impl Default for ModelPickerConfig {
    fn default() -> Self {
        Self {
            host_adapter: HostAdapterKind::Direct,
            base_url: DEFAULT_GROK_BASE_URL.into(),
            action_path: DEFAULT_GROK_ACTION_PATH.into(),
            action_path_auto: true,
            sync_native_auth: true,
            sync_native_sessions: false,
            credential_present: false,
            renderer_addons: Vec::new(),
            renderer_addon_catalog: Vec::new(),
            renderer_addon_reports: Vec::new(),
            control_nonce: generate_control_nonce(),
        }
    }
}

impl ModelPickerConfig {
    fn validate(&self) -> Result<()> {
        provider_base_url_for_action_path(&self.base_url, &self.action_path)?;
        if self.renderer_addons.len() > 16
            || self.renderer_addon_catalog.len() > 16
            || self.renderer_addon_reports.len() > 16
        {
            bail!("model picker contains too many renderer addons");
        }
        let mut addon_ids = BTreeSet::new();
        for addon in &self.renderer_addons {
            addon.validate()?;
            if !addon_ids.insert(addon.id.as_str()) {
                bail!("model picker contains duplicate renderer addons");
            }
        }
        let mut catalog_ids = BTreeSet::new();
        for addon in &self.renderer_addon_catalog {
            if !catalog_ids.insert(addon.id.as_str()) {
                bail!("model picker contains duplicate renderer addon catalog entries");
            }
        }
        if self.control_nonce.len() != 64
            || !self
                .control_nonce
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("model picker control nonce is invalid");
        }
        Ok(())
    }
}

fn generate_control_nonce() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = CONTROL_NONCE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut hasher = Sha256::new();
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(now.to_le_bytes());
    hasher.update(sequence.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Serialize)]
struct SerializedBootstrapConfig<'a> {
    version: u8,
    provider_id: &'static str,
    models: Vec<SerializedInjectedModel<'a>>,
    model_picker: &'a ModelPickerConfig,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedInjectedModel<'a> {
    id: &'a str,
    model: &'a str,
    upgrade: Option<&'a str>,
    upgrade_info: Option<&'a str>,
    availability_nux: Option<&'a str>,
    display_name: &'a str,
    description: &'a str,
    hidden: bool,
    supported_reasoning_efforts: Vec<SerializedReasoningEffort<'a>>,
    default_reasoning_effort: &'a str,
    input_modalities: &'a [String],
    supports_personality: bool,
    additional_speed_tiers: Vec<&'a str>,
    service_tiers: Vec<&'a str>,
    default_service_tier: Option<&'a str>,
    is_default: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SerializedReasoningEffort<'a> {
    reasoning_effort: &'a str,
    description: &'a str,
}

pub fn render_bootstrap(config: &BootstrapConfig) -> Result<String> {
    if config.models.len() > 128 {
        bail!("bootstrap supports at most 128 injected models");
    }
    let mut model_ids = BTreeSet::new();
    for model in &config.models {
        model.validate()?;
        if !model_ids.insert(model.model.as_str()) {
            bail!("bootstrap contains a duplicate injected model id");
        }
    }
    config.model_picker.validate()?;

    let models = config
        .models
        .iter()
        .map(|model| SerializedInjectedModel {
            id: &model.model,
            model: &model.model,
            upgrade: None,
            upgrade_info: None,
            availability_nux: None,
            display_name: &model.display_name,
            description: &model.description,
            hidden: false,
            supported_reasoning_efforts: model
                .supported_reasoning_efforts
                .iter()
                .map(|effort| SerializedReasoningEffort {
                    reasoning_effort: &effort.reasoning_effort,
                    description: &effort.description,
                })
                .collect(),
            default_reasoning_effort: &model.default_reasoning_effort,
            input_modalities: &model.input_modalities,
            supports_personality: false,
            additional_speed_tiers: Vec::new(),
            service_tiers: Vec::new(),
            default_service_tier: None,
            is_default: false,
        })
        .collect();
    let serialized = serde_json::to_string(&SerializedBootstrapConfig {
        version: 2,
        provider_id: GROK_NATIVE_PROVIDER_ID,
        models,
        model_picker: &config.model_picker,
    })?;
    let bootstrap = BOOTSTRAP_TEMPLATE.replace("/*__CODEX_ADMINISTRATOR_CONFIG__*/", &serialized);
    Ok(format!(
        "{RENDERER_API_DISCOVERY}\n{MODEL_INJECTION_CORE}\n{MODEL_PICKER_MOUNT}\n{bootstrap}"
    ))
}

fn validate_text(value: &str, field: &str, max_len: usize, allow_empty: bool) -> Result<()> {
    let trimmed = value.trim();
    if value != trimmed
        || (!allow_empty && trimmed.is_empty())
        || value.len() > max_len
        || value.chars().any(char::is_control)
    {
        bail!("{field} is invalid");
    }
    Ok(())
}
