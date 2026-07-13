use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::GROK_NATIVE_PROVIDER_ID;

const MODEL_INJECTION_CORE: &str = include_str!("../assets/model-injection-core.js");
const BOOTSTRAP_TEMPLATE: &str = include_str!("../assets/bootstrap.js");

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
        Self {
            display_name: model.clone(),
            description: "Configured xAI model; new tasks only".into(),
            model,
            supported_reasoning_efforts: vec![InjectedReasoningEffort {
                reasoning_effort: "medium".into(),
                description: "Balanced reasoning".into(),
            }],
            default_reasoning_effort: "medium".into(),
            input_modalities: vec!["text".into()],
        }
    }

    fn validate(&self) -> Result<()> {
        validate_text(&self.model, "injected model id", 256, false)?;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapConfig {
    pub models: Vec<InjectedModelDescriptor>,
}

#[derive(Serialize)]
struct SerializedBootstrapConfig<'a> {
    version: u8,
    provider_id: &'static str,
    models: Vec<SerializedInjectedModel<'a>>,
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
    if config.models.is_empty() || config.models.len() > 128 {
        bail!("bootstrap requires 1-128 injected models");
    }
    let mut model_ids = BTreeSet::new();
    for model in &config.models {
        model.validate()?;
        if !model_ids.insert(model.model.as_str()) {
            bail!("bootstrap contains a duplicate injected model id");
        }
    }

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
    })?;
    let bootstrap = BOOTSTRAP_TEMPLATE.replace("/*__CODEX_ADMINISTRATOR_CONFIG__*/", &serialized);
    Ok(format!("{MODEL_INJECTION_CORE}\n{bootstrap}"))
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
