use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::host::HostAdapterKind;

const MAX_ADDONS: usize = 16;
const MAX_ASSETS_PER_ADDON: usize = 32;
const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
const RENDERER_ADDON_RUNTIME: &str = include_str!("../assets/renderer-addon-runtime.js");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RendererAddonSettings {
    pub id: String,
    pub enabled: bool,
    pub source_root: PathBuf,
}

impl RendererAddonSettings {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        if self.enabled && !self.source_root.is_absolute() {
            bail!("enabled renderer addon source root must be absolute");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererAddonState {
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RendererAddonReport {
    pub id: String,
    pub state: RendererAddonState,
    pub project_revision: Option<String>,
    pub reason: Option<String>,
    pub blocked_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RendererAddonCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub project_revision: String,
}

#[derive(Debug, Clone)]
struct PreparedRendererAddon {
    id: String,
    project_revision: String,
    script: String,
    lifecycle: RendererAddonLifecycle,
}

#[derive(Debug, Clone, Default)]
pub struct RendererAddonBundle {
    scripts: Vec<PreparedRendererAddon>,
    reports: Vec<RendererAddonReport>,
}

impl RendererAddonBundle {
    pub fn reports(&self) -> &[RendererAddonReport] {
        &self.reports
    }

    pub fn compose(&self, primary: &str) -> String {
        let mut composed = format!(
            "{primary}\n/* Codex Administrator renderer addon runtime */\n{RENDERER_ADDON_RUNTIME}\n"
        );
        for addon in &self.scripts {
            let descriptor = serde_json::to_string(&RendererAddonRuntimeDescriptor {
                id: &addon.id,
                revision: &addon.project_revision,
                state_key: &addon.lifecycle.state_key,
                dispose_method: &addon.lifecycle.dispose_method,
            })
            .expect("validated renderer addon runtime descriptor must serialize");
            composed.push_str(&format!(
                "\n/* Codex Administrator renderer addon: {} */\n\
                 globalThis.__codexAdministratorRendererAddons.apply({descriptor}, () => {{\n{}\n}});\n",
                addon.id, addon.script
            ));
        }
        composed
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RendererAddonRuntimeDescriptor<'a> {
    id: &'a str,
    revision: &'a str,
    state_key: &'a str,
    dispose_method: &'a str,
}

#[derive(Debug, Clone)]
pub struct RendererAddonPolicy {
    entries: BTreeMap<String, RendererAddonRevision>,
}

impl RendererAddonPolicy {
    pub fn shipped() -> Result<Self> {
        Self::from_json(include_bytes!("../renderer-addons.json"))
    }

    pub fn from_json(content: &[u8]) -> Result<Self> {
        let manifest: RendererAddonManifest =
            serde_json::from_slice(content).context("invalid renderer addon manifest")?;
        if manifest.schema_version != 2 {
            bail!(
                "unsupported renderer addon manifest schema version {}",
                manifest.schema_version
            );
        }
        if manifest.addons.len() > MAX_ADDONS {
            bail!("renderer addon manifest contains too many entries");
        }
        let mut entries = BTreeMap::new();
        for addon in manifest.addons {
            addon.validate()?;
            if entries.insert(addon.id.clone(), addon).is_some() {
                bail!("renderer addon manifest contains duplicate IDs");
            }
        }
        Ok(Self { entries })
    }

    pub fn catalog(&self, host: HostAdapterKind) -> Vec<RendererAddonCatalogEntry> {
        let mut entries = self
            .entries
            .values()
            .filter(|entry| entry.supports_host(host))
            .map(|entry| RendererAddonCatalogEntry {
                id: entry.id.clone(),
                display_name: entry.display_name.clone(),
                project_revision: entry.project_revision.clone(),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.id.cmp(&right.id));
        entries
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererAddonManifest {
    schema_version: u8,
    addons: Vec<RendererAddonRevision>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererAddonRevision {
    id: String,
    display_name: String,
    project_revision: String,
    distribution: String,
    load_order: u16,
    host_adapters: Vec<HostAdapterKind>,
    exclusive_slots: Vec<String>,
    conflicts_with: Vec<String>,
    entrypoint: RendererAddonAsset,
    substitutions: Vec<RendererAddonSubstitution>,
    lifecycle: RendererAddonLifecycle,
}

impl RendererAddonRevision {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text(&self.display_name, "renderer addon display name", 96)?;
        validate_text(
            &self.project_revision,
            "renderer addon project revision",
            128,
        )?;
        if self.distribution != "external_checkout_only" {
            bail!("renderer addon assets must remain in an external checkout");
        }
        if self.host_adapters.is_empty() {
            bail!("renderer addon must support at least one host adapter");
        }
        let mut hosts = BTreeSet::new();
        if !self.host_adapters.iter().all(|host| hosts.insert(*host)) {
            bail!("renderer addon contains duplicate host adapters");
        }
        validate_id_collection(&self.exclusive_slots, "exclusive slots")?;
        validate_id_collection(&self.conflicts_with, "conflicts")?;
        if self.conflicts_with.iter().any(|id| id == &self.id) {
            bail!("renderer addon cannot conflict with itself");
        }
        self.entrypoint.validate()?;
        if self.substitutions.len() > MAX_ASSETS_PER_ADDON {
            bail!("renderer addon contains too many asset substitutions");
        }
        let mut placeholders = BTreeSet::new();
        for substitution in &self.substitutions {
            substitution.validate()?;
            if !placeholders.insert(substitution.placeholder.as_str()) {
                bail!("renderer addon contains duplicate asset placeholders");
            }
        }
        self.lifecycle.validate()?;
        Ok(())
    }

    fn supports_host(&self, host: HostAdapterKind) -> bool {
        self.host_adapters.contains(&host)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererAddonLifecycle {
    state_key: String,
    dispose_method: String,
}

impl RendererAddonLifecycle {
    fn validate(&self) -> Result<()> {
        validate_js_identifier(&self.state_key, "renderer addon lifecycle state key")?;
        validate_js_identifier(
            &self.dispose_method,
            "renderer addon lifecycle dispose method",
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererAddonSubstitution {
    placeholder: String,
    encoding: RendererAddonAssetEncoding,
    #[serde(default)]
    media_type: Option<String>,
    asset: RendererAddonAsset,
}

impl RendererAddonSubstitution {
    fn validate(&self) -> Result<()> {
        validate_placeholder(&self.placeholder)?;
        self.asset.validate()?;
        match self.encoding {
            RendererAddonAssetEncoding::JsonUtf8 => {
                if self.media_type.is_some() {
                    bail!("UTF-8 renderer addon assets cannot declare a media type");
                }
            }
            RendererAddonAssetEncoding::DataUrlBase64 => {
                validate_media_type(self.media_type.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("data URL renderer addon asset requires a media type")
                })?)?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RendererAddonAssetEncoding {
    JsonUtf8,
    DataUrlBase64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererAddonAsset {
    path: PathBuf,
    sha256: String,
    max_bytes: u64,
}

impl RendererAddonAsset {
    fn validate(&self) -> Result<()> {
        validate_relative_path(&self.path)?;
        normalize_sha256(&self.sha256)?;
        if self.max_bytes == 0 || self.max_bytes > MAX_ASSET_BYTES {
            bail!("renderer addon asset size limit is invalid");
        }
        Ok(())
    }
}

struct Candidate<'a> {
    setting: &'a RendererAddonSettings,
    revision: &'a RendererAddonRevision,
}

pub fn prepare_renderer_addons(
    settings: &[RendererAddonSettings],
    policy: &RendererAddonPolicy,
    host: HostAdapterKind,
) -> RendererAddonBundle {
    let mut bundle = RendererAddonBundle::default();
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    for setting in settings.iter().take(MAX_ADDONS) {
        if !seen.insert(setting.id.as_str()) {
            bundle
                .reports
                .push(disabled_report(&setting.id, None, "duplicate_addon", None));
        } else if setting.validate().is_err() {
            bundle
                .reports
                .push(disabled_report(&setting.id, None, "invalid_settings", None));
        } else if !setting.enabled {
            bundle
                .reports
                .push(disabled_report(&setting.id, None, "disabled_by_user", None));
        } else if let Some(revision) = policy.entries.get(&setting.id) {
            if revision.supports_host(host) {
                candidates.push(Candidate { setting, revision });
            } else {
                bundle.reports.push(disabled_report(
                    &setting.id,
                    Some(&revision.project_revision),
                    "host_adapter_unsupported",
                    None,
                ));
            }
        } else {
            bundle
                .reports
                .push(disabled_report(&setting.id, None, "unreviewed_addon", None));
        }
    }
    if settings.len() > MAX_ADDONS {
        bundle.reports.push(disabled_report(
            "additional-addons",
            None,
            "too_many_addons",
            None,
        ));
    }

    candidates.sort_by(|left, right| {
        left.revision
            .load_order
            .cmp(&right.revision.load_order)
            .then_with(|| left.revision.id.cmp(&right.revision.id))
    });
    let mut active_revisions = Vec::<&RendererAddonRevision>::new();
    let mut occupied_slots = BTreeMap::<&str, &str>::new();
    for candidate in candidates {
        if let Some(blocker) = explicit_conflict(candidate.revision, &active_revisions) {
            bundle.reports.push(disabled_report(
                &candidate.setting.id,
                Some(&candidate.revision.project_revision),
                "addon_conflict",
                Some(blocker),
            ));
            continue;
        }
        if let Some(blocker) = candidate
            .revision
            .exclusive_slots
            .iter()
            .find_map(|slot| occupied_slots.get(slot.as_str()).copied())
        {
            bundle.reports.push(disabled_report(
                &candidate.setting.id,
                Some(&candidate.revision.project_revision),
                "exclusive_slot_conflict",
                Some(blocker),
            ));
            continue;
        }
        match load_addon_script(&candidate.setting.source_root, candidate.revision) {
            Ok(script) => {
                for slot in &candidate.revision.exclusive_slots {
                    occupied_slots.insert(slot, candidate.revision.id.as_str());
                }
                active_revisions.push(candidate.revision);
                bundle.scripts.push(PreparedRendererAddon {
                    id: candidate.setting.id.clone(),
                    project_revision: candidate.revision.project_revision.clone(),
                    script,
                    lifecycle: candidate.revision.lifecycle.clone(),
                });
                bundle.reports.push(RendererAddonReport {
                    id: candidate.setting.id.clone(),
                    state: RendererAddonState::Enabled,
                    project_revision: Some(candidate.revision.project_revision.clone()),
                    reason: None,
                    blocked_by: None,
                });
            }
            Err(error) => bundle.reports.push(disabled_report(
                &candidate.setting.id,
                Some(&candidate.revision.project_revision),
                error.reason(),
                None,
            )),
        }
    }
    bundle
}

fn explicit_conflict<'a>(
    candidate: &'a RendererAddonRevision,
    active: &[&'a RendererAddonRevision],
) -> Option<&'a str> {
    active.iter().find_map(|entry| {
        (candidate.conflicts_with.contains(&entry.id)
            || entry.conflicts_with.contains(&candidate.id))
        .then_some(entry.id.as_str())
    })
}

fn disabled_report(
    id: &str,
    revision: Option<&str>,
    reason: &str,
    blocked_by: Option<&str>,
) -> RendererAddonReport {
    RendererAddonReport {
        id: id.to_owned(),
        state: RendererAddonState::Disabled,
        project_revision: revision.map(str::to_owned),
        reason: Some(reason.to_owned()),
        blocked_by: blocked_by.map(str::to_owned),
    }
}

#[derive(Debug)]
enum AddonLoadError {
    SourceUnavailable,
    SourceIdentityMismatch,
    InvalidEntrypoint,
    InvalidSubstitution,
}

impl AddonLoadError {
    const fn reason(&self) -> &'static str {
        match self {
            Self::SourceUnavailable => "source_unavailable",
            Self::SourceIdentityMismatch => "source_identity_mismatch",
            Self::InvalidEntrypoint => "invalid_entrypoint",
            Self::InvalidSubstitution => "invalid_substitution",
        }
    }
}

fn load_addon_script(
    root: &Path,
    revision: &RendererAddonRevision,
) -> std::result::Result<String, AddonLoadError> {
    let entrypoint = read_reviewed_asset(root, &revision.entrypoint)?;
    let mut script =
        String::from_utf8(entrypoint).map_err(|_| AddonLoadError::InvalidEntrypoint)?;
    for substitution in &revision.substitutions {
        if script.matches(&substitution.placeholder).count() != 1 {
            return Err(AddonLoadError::InvalidSubstitution);
        }
        let content = read_reviewed_asset(root, &substitution.asset)?;
        let replacement = match substitution.encoding {
            RendererAddonAssetEncoding::JsonUtf8 => {
                let text =
                    String::from_utf8(content).map_err(|_| AddonLoadError::InvalidSubstitution)?;
                serde_json::to_string(&text).map_err(|_| AddonLoadError::InvalidSubstitution)?
            }
            RendererAddonAssetEncoding::DataUrlBase64 => serde_json::to_string(&format!(
                "data:{};base64,{}",
                substitution
                    .media_type
                    .as_deref()
                    .ok_or(AddonLoadError::InvalidSubstitution)?,
                STANDARD.encode(content)
            ))
            .map_err(|_| AddonLoadError::InvalidSubstitution)?,
        };
        script = script.replacen(&substitution.placeholder, &replacement, 1);
    }
    Ok(script)
}

fn read_reviewed_asset(
    root: &Path,
    asset: &RendererAddonAsset,
) -> std::result::Result<Vec<u8>, AddonLoadError> {
    if !root.is_absolute() {
        return Err(AddonLoadError::SourceUnavailable);
    }
    let canonical_root = fs::canonicalize(root).map_err(|_| AddonLoadError::SourceUnavailable)?;
    let candidate = root.join(&asset.path);
    let canonical_candidate =
        fs::canonicalize(&candidate).map_err(|_| AddonLoadError::SourceUnavailable)?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(AddonLoadError::SourceUnavailable);
    }
    let metadata =
        fs::metadata(&canonical_candidate).map_err(|_| AddonLoadError::SourceUnavailable)?;
    if !metadata.is_file() || metadata.len() > asset.max_bytes {
        return Err(AddonLoadError::SourceUnavailable);
    }
    let content = fs::read(&canonical_candidate).map_err(|_| AddonLoadError::SourceUnavailable)?;
    let observed = format!("{:x}", Sha256::digest(&content));
    if observed != asset.sha256.to_ascii_lowercase() {
        return Err(AddonLoadError::SourceIdentityMismatch);
    }
    Ok(content)
}

fn validate_id_collection(values: &[String], field: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_id(value)?;
        if !seen.insert(value.as_str()) {
            bail!("renderer addon contains duplicate {field}");
        }
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
    {
        bail!("renderer addon id is invalid");
    }
    Ok(())
}

fn validate_text(value: &str, field: &str, max_len: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        bail!("{field} is invalid");
    }
    Ok(())
}

fn validate_placeholder(value: &str) -> Result<()> {
    if value.len() < 5
        || value.len() > 128
        || !value.starts_with("__")
        || !value.ends_with("__")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        bail!("renderer addon asset placeholder is invalid");
    }
    Ok(())
}

fn validate_js_identifier(value: &str, field: &str) -> Result<()> {
    let mut bytes = value.bytes();
    let first = bytes
        .next()
        .ok_or_else(|| anyhow::anyhow!("{field} is invalid"))?;
    if value.len() > 128
        || !matches!(first, b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$')
        || !bytes.all(|byte| matches!(byte, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'$'))
    {
        bail!("{field} is invalid");
    }
    Ok(())
}

fn validate_media_type(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 96
        || value.trim() != value
        || value.matches('/').count() != 1
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'/' | b'-' | b'+' | b'.')
        })
    {
        bail!("renderer addon media type is invalid");
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("renderer addon asset path must stay relative to its checkout");
    }
    Ok(())
}

fn normalize_sha256(value: &str) -> Result<String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("renderer addon asset identity must be a SHA-256 value");
    }
    Ok(value.to_ascii_lowercase())
}
