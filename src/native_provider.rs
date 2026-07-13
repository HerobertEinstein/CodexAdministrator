use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use axum::http::Uri;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::{RuntimeKind, RuntimeLaunchSpec, install_bootstrap_atomically};

pub const GROK_NATIVE_PROVIDER_ID: &str = "grok_native";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NativeProviderCapabilities {
    pub responses: bool,
    pub streaming: bool,
    pub tool_calls: bool,
    pub parallel_tool_calls: bool,
    pub image_input: bool,
    pub file_input: bool,
    pub structured_outputs: bool,
    pub reasoning_summaries: bool,
    pub websockets: bool,
}

impl NativeProviderCapabilities {
    pub const fn native_codex_agent_ready(self) -> bool {
        self.responses && self.streaming && self.tool_calls
    }

    pub const fn multimodal_ready(self) -> bool {
        self.image_input && self.file_input
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProviderCapabilityManifest {
    schema_version: u8,
    provider_id: String,
    models: Vec<String>,
    pub capabilities: NativeProviderCapabilities,
    evidence_sha256: String,
}

impl NativeProviderCapabilityManifest {
    pub fn from_json(content: &[u8]) -> Result<Self> {
        let manifest: Self = serde_json::from_slice(content)
            .context("invalid native provider capability manifest")?;
        if manifest.schema_version != 1 {
            bail!(
                "unsupported native provider capability schema version {}",
                manifest.schema_version
            );
        }
        if manifest.provider_id != GROK_NATIVE_PROVIDER_ID {
            bail!(
                "native provider capability manifest targets {}, expected {}",
                manifest.provider_id,
                GROK_NATIVE_PROVIDER_ID
            );
        }
        if manifest.models.is_empty() || manifest.models.len() > 1024 {
            bail!("native provider capability manifest must name 1-1024 models");
        }
        let mut unique = std::collections::BTreeSet::new();
        for model in &manifest.models {
            let trimmed = model.trim();
            if model != trimmed
                || trimmed.is_empty()
                || trimmed.len() > 256
                || trimmed.chars().any(char::is_control)
            {
                bail!("native provider capability manifest contains an invalid model id");
            }
            if !unique.insert(trimmed) {
                bail!("native provider capability manifest contains a duplicate model id");
            }
        }
        let evidence = manifest.evidence_sha256.as_bytes();
        if evidence.len() != 64 || !evidence.iter().all(u8::is_ascii_hexdigit) {
            bail!("native provider capability evidence must be a SHA-256 digest");
        }
        Ok(manifest)
    }

    pub fn supports_model(&self, model: &str) -> bool {
        let model = model.trim();
        self.models.iter().any(|candidate| candidate == model)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokNativeProviderConfig {
    pub base_url: String,
    pub env_key: String,
    pub supports_websockets: bool,
}

impl GrokNativeProviderConfig {
    pub fn validate(&self) -> Result<()> {
        let uri: Uri = self
            .base_url
            .parse()
            .context("Grok native provider base URL is invalid")?;
        let scheme = uri
            .scheme_str()
            .ok_or_else(|| anyhow::anyhow!("Grok native provider URL requires a scheme"))?;
        let host = uri
            .host()
            .ok_or_else(|| anyhow::anyhow!("Grok native provider URL requires a host"))?;
        let loopback = matches!(host, "127.0.0.1" | "localhost" | "::1");
        if scheme != "https" && !(scheme == "http" && loopback) {
            bail!("remote Grok native providers must use HTTPS");
        }
        if !uri.path().trim_end_matches('/').ends_with("/v1") {
            bail!("Grok native provider base URL must end in /v1");
        }
        if uri.query().is_some() {
            bail!("Grok native provider base URL must not contain a query string");
        }
        let env_key = self.env_key.as_bytes();
        if env_key.is_empty()
            || env_key.len() > 128
            || !matches!(env_key[0], b'A'..=b'Z' | b'_')
            || !env_key
                .iter()
                .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b'_'))
        {
            bail!("Grok native provider env_key must be an uppercase environment variable name");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeProviderInstallReceipt {
    pub updated: bool,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSelectionBackup {
    schema_version: u8,
    managed_model: String,
    managed_model_catalog_json: Option<String>,
    previous_model: Option<String>,
    previous_model_provider: Option<String>,
    previous_model_catalog_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexNativeAppLaunchSpec {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub use_shell: bool,
}

pub fn build_codex_native_app_launch(
    runtime: &RuntimeLaunchSpec,
    workspace: &Path,
) -> Result<CodexNativeAppLaunchSpec> {
    if runtime.kind != RuntimeKind::Codex || runtime.use_shell {
        bail!("native app launch requires the official Codex runtime without a shell");
    }
    if !workspace.is_absolute() {
        bail!("Codex app workspace path must be absolute");
    }

    let mut args = Vec::new();
    if let Some(script) = runtime.args.first().filter(|value| {
        Path::new(value)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("codex.js"))
    }) {
        args.push(script.clone());
    }
    args.push("app".into());
    args.push(workspace.to_string_lossy().into_owned());

    Ok(CodexNativeAppLaunchSpec {
        executable: runtime.executable.clone(),
        args,
        use_shell: false,
    })
}

pub fn validate_codex_model_catalog(path: &Path, model: &str) -> Result<()> {
    let metadata = fs::metadata(path).with_context(|| {
        format!(
            "failed to read Codex model catalog metadata {}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        bail!("Codex model catalog is not a file: {}", path.display());
    }
    if metadata.len() > 16 * 1024 * 1024 {
        bail!("Codex model catalog exceeds the 16 MiB validation limit");
    }
    let content = fs::read(path)
        .with_context(|| format!("failed to read Codex model catalog {}", path.display()))?;
    let catalog: serde_json::Value = serde_json::from_slice(&content)
        .with_context(|| format!("failed to parse Codex model catalog {}", path.display()))?;
    let models = catalog
        .get("models")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Codex model catalog must contain a models array"))?;
    if models.is_empty() {
        bail!("Codex model catalog must contain at least one model");
    }
    for entry in models {
        validate_codex_model_catalog_entry(entry)?;
    }
    if !models
        .iter()
        .any(|entry| entry.get("slug").and_then(serde_json::Value::as_str) == Some(model.trim()))
    {
        bail!("Codex model catalog does not contain selected model {model}");
    }
    Ok(())
}

pub fn validate_codex_model_catalog_with_runtime(
    runtime: &RuntimeLaunchSpec,
    path: &Path,
    model: &str,
) -> Result<()> {
    validate_codex_model_catalog(path, model)?;
    if runtime.kind != RuntimeKind::Codex || runtime.use_shell {
        bail!("model catalog validation requires the official Codex runtime without a shell");
    }
    let mut args = Vec::new();
    if let Some(script) = runtime.args.first().filter(|value| {
        Path::new(value)
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("codex.js"))
    }) {
        args.push(script.clone());
    }
    args.extend([
        "-c".into(),
        format!(
            "model_catalog_json={}",
            serde_json::to_string(&path.to_string_lossy())?
        ),
        "debug".into(),
        "models".into(),
    ]);
    let output = Command::new(&runtime.executable)
        .args(args)
        .envs(&runtime.env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| {
            format!(
                "failed to run official Codex model catalog validation via {}",
                runtime.executable.display()
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(2048)
            .collect::<String>();
        bail!(
            "official Codex rejected model catalog {}: {}",
            path.display(),
            stderr.trim()
        );
    }
    Ok(())
}

fn validate_codex_model_catalog_entry(entry: &serde_json::Value) -> Result<()> {
    let object = entry
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Codex model catalog entries must be JSON objects"))?;
    for key in [
        "slug",
        "display_name",
        "description",
        "supported_reasoning_levels",
        "shell_type",
        "visibility",
        "supported_in_api",
        "priority",
        "availability_nux",
        "upgrade",
        "base_instructions",
        "supports_reasoning_summaries",
        "support_verbosity",
        "default_verbosity",
        "apply_patch_tool_type",
        "truncation_policy",
        "supports_parallel_tool_calls",
        "experimental_supported_tools",
    ] {
        if !object.contains_key(key) {
            bail!("Codex model catalog entry is missing required field {key}");
        }
    }
    if object
        .get("slug")
        .and_then(serde_json::Value::as_str)
        .is_none()
        || object
            .get("display_name")
            .and_then(serde_json::Value::as_str)
            .is_none()
        || object
            .get("base_instructions")
            .and_then(serde_json::Value::as_str)
            .is_none()
        || object
            .get("supported_reasoning_levels")
            .and_then(serde_json::Value::as_array)
            .is_none()
        || object
            .get("experimental_supported_tools")
            .and_then(serde_json::Value::as_array)
            .is_none()
    {
        bail!("Codex model catalog entry has an invalid required field type");
    }
    Ok(())
}

pub fn install_grok_native_provider(
    config_path: &Path,
    provider: &GrokNativeProviderConfig,
) -> Result<NativeProviderInstallReceipt> {
    install_grok_native_provider_inner(config_path, provider, None, None)
}

pub fn install_grok_native_provider_for_model(
    config_path: &Path,
    provider: &GrokNativeProviderConfig,
    model: &str,
    model_catalog: Option<&Path>,
) -> Result<NativeProviderInstallReceipt> {
    let model = validate_model_id(model)?;
    if let Some(model_catalog) = model_catalog {
        if !model_catalog.is_absolute() {
            bail!("Codex model catalog path must be absolute");
        }
        validate_codex_model_catalog(model_catalog, model)?;
    }
    install_grok_native_provider_inner(config_path, provider, Some(model), model_catalog)
}

fn install_grok_native_provider_inner(
    config_path: &Path,
    provider: &GrokNativeProviderConfig,
    model: Option<&str>,
    model_catalog: Option<&Path>,
) -> Result<NativeProviderInstallReceipt> {
    provider.validate()?;
    let existing = match fs::read_to_string(config_path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    let mut document = if existing.trim().is_empty() {
        DocumentMut::new()
    } else {
        existing
            .parse::<DocumentMut>()
            .with_context(|| format!("failed to parse {}", config_path.display()))?
    };

    if let Some(model) = model {
        if model_catalog.is_none() && document.contains_key("model_catalog_json") {
            bail!(
                "existing model_catalog_json requires an explicit reviewed catalog containing {model}"
            );
        }
        persist_native_selection_backup(config_path, &document, model, model_catalog)?;
    }

    if !document.contains_key("model_providers") {
        document["model_providers"] = Item::Table(Table::new());
    }
    let providers = document["model_providers"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("model_providers must be a TOML table"))?;
    if !providers.contains_key(GROK_NATIVE_PROVIDER_ID) {
        providers[GROK_NATIVE_PROVIDER_ID] = Item::Table(Table::new());
    }
    let table = providers[GROK_NATIVE_PROVIDER_ID]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("model_providers.grok_native must be a TOML table"))?;
    table["name"] = value("Grok in native ChatGPT/Codex");
    table["base_url"] = value(provider.base_url.as_str());
    table["env_key"] = value(provider.env_key.as_str());
    table["wire_api"] = value("responses");
    table["requires_openai_auth"] = value(false);
    table["supports_websockets"] = value(provider.supports_websockets);

    if let Some(model) = model {
        document["model_provider"] = value(GROK_NATIVE_PROVIDER_ID);
        document["model"] = value(model);
        if let Some(model_catalog) = model_catalog {
            document["model_catalog_json"] = value(model_catalog.to_string_lossy().into_owned());
        }
    }

    let rendered = document.to_string();
    let updated = rendered != existing;
    let sha256 = if updated {
        install_bootstrap_atomically(config_path, rendered.as_bytes())?
    } else {
        format!("{:x}", sha2::Sha256::digest(rendered.as_bytes()))
    };
    Ok(NativeProviderInstallReceipt { updated, sha256 })
}

fn validate_model_id(model: &str) -> Result<&str> {
    let trimmed = model.trim();
    if model != trimmed
        || trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed.chars().any(char::is_control)
    {
        bail!("Grok model id must be 1-256 printable characters");
    }
    Ok(trimmed)
}

pub fn restore_native_model_selection(config_path: &Path) -> Result<NativeProviderInstallReceipt> {
    let backup_path = native_selection_backup_path(config_path)?;
    let backup_content = fs::read(&backup_path).with_context(|| {
        format!(
            "failed to read native selection backup {}",
            backup_path.display()
        )
    })?;
    let backup: NativeSelectionBackup =
        serde_json::from_slice(&backup_content).context("invalid native selection backup")?;
    if backup.schema_version != 1 {
        bail!(
            "unsupported native selection backup schema version {}",
            backup.schema_version
        );
    }

    let existing = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let mut document = existing
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    let current_provider = optional_string(&document, "model_provider")?;
    let current_model = optional_string(&document, "model")?;
    let current_catalog = optional_string(&document, "model_catalog_json")?;
    if current_provider.as_deref() != Some(GROK_NATIVE_PROVIDER_ID)
        || current_model.as_deref() != Some(backup.managed_model.as_str())
        || current_catalog != backup.managed_model_catalog_json
    {
        bail!(
            "current Codex model selection changed after Grok activation; refusing to overwrite it"
        );
    }

    restore_optional_string(&mut document, "model", backup.previous_model.as_deref());
    restore_optional_string(
        &mut document,
        "model_provider",
        backup.previous_model_provider.as_deref(),
    );
    restore_optional_string(
        &mut document,
        "model_catalog_json",
        backup.previous_model_catalog_json.as_deref(),
    );
    let rendered = document.to_string();
    let updated = rendered != existing;
    let sha256 = if updated {
        install_bootstrap_atomically(config_path, rendered.as_bytes())?
    } else {
        format!("{:x}", sha2::Sha256::digest(rendered.as_bytes()))
    };
    fs::remove_file(&backup_path).with_context(|| {
        format!(
            "restored native selection but failed to remove backup {}",
            backup_path.display()
        )
    })?;
    if let Some(parent) = backup_path.parent() {
        let _ = fs::remove_dir(parent);
    }
    Ok(NativeProviderInstallReceipt { updated, sha256 })
}

fn persist_native_selection_backup(
    config_path: &Path,
    document: &DocumentMut,
    model: &str,
    model_catalog: Option<&Path>,
) -> Result<()> {
    let backup_path = native_selection_backup_path(config_path)?;
    let existing_backup = match fs::read(&backup_path) {
        Ok(content) => {
            let backup = serde_json::from_slice::<NativeSelectionBackup>(&content)
                .context("invalid native selection backup")?;
            if backup.schema_version != 1 {
                bail!(
                    "unsupported native selection backup schema version {}",
                    backup.schema_version
                );
            }
            Some(backup)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to read native selection backup {}",
                    backup_path.display()
                )
            });
        }
    };
    let current_provider = optional_string(document, "model_provider")?;
    let preserve_previous = current_provider.as_deref() == Some(GROK_NATIVE_PROVIDER_ID);
    if preserve_previous && existing_backup.is_none() {
        bail!("Grok is already selected but its native selection backup is missing");
    }
    let current_model = optional_string(document, "model")?;
    let current_catalog = optional_string(document, "model_catalog_json")?;
    let (previous_model, previous_model_provider, previous_model_catalog_json) =
        if preserve_previous {
            if let Some(existing_backup) = existing_backup {
                (
                    existing_backup.previous_model,
                    existing_backup.previous_model_provider,
                    existing_backup.previous_model_catalog_json,
                )
            } else {
                (current_model, current_provider, current_catalog)
            }
        } else {
            (current_model, current_provider, current_catalog)
        };
    let backup = NativeSelectionBackup {
        schema_version: 1,
        managed_model: model.to_owned(),
        managed_model_catalog_json: model_catalog.map(|path| path.to_string_lossy().into_owned()),
        previous_model,
        previous_model_provider,
        previous_model_catalog_json,
    };
    let content = serde_json::to_vec_pretty(&backup)?;
    install_bootstrap_atomically(&backup_path, &content)?;
    Ok(())
}

fn native_selection_backup_path(config_path: &Path) -> Result<PathBuf> {
    let parent = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Codex config path has no parent"))?;
    Ok(parent
        .join("codex-administrator")
        .join("native-selection-backup.json"))
}

fn optional_string(document: &DocumentMut, key: &str) -> Result<Option<String>> {
    document
        .get(key)
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("Codex config field {key} must be a string"))
        })
        .transpose()
}

fn restore_optional_string(document: &mut DocumentMut, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        document[key] = toml_edit::value(value);
    } else {
        document.remove(key);
    }
}
