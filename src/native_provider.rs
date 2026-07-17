use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use http::Uri;
use sha2::Digest;
use toml_edit::{Array, DocumentMut, Item, Table, Value, value};

use crate::{install_bootstrap_atomically, provider_base_url_for_action_path};

pub const GROK_NATIVE_PROVIDER_ID: &str = "grok_native";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokNativeProviderConfig {
    pub base_url: String,
    pub action_path: String,
    pub env_key: String,
    pub supports_websockets: bool,
}

impl GrokNativeProviderConfig {
    pub fn validate(&self) -> Result<()> {
        provider_base_url_for_action_path(&self.base_url, &self.action_path)?;
        let uri: Uri = self
            .base_url
            .parse()
            .context("Grok provider base URL is invalid")?;
        let scheme = uri
            .scheme_str()
            .ok_or_else(|| anyhow::anyhow!("Grok provider URL requires a scheme"))?;
        let authority = uri
            .authority()
            .ok_or_else(|| anyhow::anyhow!("Grok provider URL requires an authority"))?;
        let host = uri
            .host()
            .ok_or_else(|| anyhow::anyhow!("Grok provider URL requires a host"))?;
        let loopback = matches!(host, "127.0.0.1" | "localhost" | "::1");
        if scheme != "https" && !(scheme == "http" && loopback) {
            bail!("remote Grok providers must use HTTPS");
        }
        if authority.as_str().contains('@') || uri.query().is_some() {
            bail!("Grok provider base URL must not contain credentials or a query");
        }

        let env_key = self.env_key.as_bytes();
        if env_key.is_empty()
            || env_key.len() > 128
            || !matches!(env_key[0], b'A'..=b'Z' | b'_')
            || !env_key
                .iter()
                .all(|byte| matches!(byte, b'A'..=b'Z' | b'0'..=b'9' | b'_'))
        {
            bail!("Grok provider env_key must be an uppercase environment variable name");
        }
        if self.env_key == "OPENAI_API_KEY" {
            bail!(
                "OPENAI_API_KEY is reserved by official host authentication and may be persisted; use a provider-specific environment variable name"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeProviderInstallReceipt {
    pub updated: bool,
    pub sha256: String,
}

pub fn install_grok_native_provider(
    config_path: &Path,
    provider: &GrokNativeProviderConfig,
) -> Result<NativeProviderInstallReceipt> {
    provider.validate()?;
    let provider_base_url =
        provider_base_url_for_action_path(&provider.base_url, &provider.action_path)?;
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
    table["base_url"] = value(provider_base_url);
    table["env_key"] = value(provider.env_key.as_str());
    table["wire_api"] = value("responses");
    table["requires_openai_auth"] = value(false);
    table["supports_websockets"] = value(provider.supports_websockets);
    install_shell_environment_guard(&mut document, &provider.env_key)?;

    let rendered = document.to_string();
    let updated = rendered != existing;
    let sha256 = if updated {
        install_bootstrap_atomically(config_path, rendered.as_bytes())?
    } else {
        format!("{:x}", sha2::Sha256::digest(rendered.as_bytes()))
    };
    Ok(NativeProviderInstallReceipt { updated, sha256 })
}

pub fn remove_grok_native_provider(config_path: &Path) -> Result<bool> {
    let existing = match fs::read_to_string(config_path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", config_path.display()));
        }
    };
    if existing.trim().is_empty() {
        return Ok(false);
    }
    let mut document = existing
        .parse::<DocumentMut>()
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    let mut changed = false;
    if let Some(providers) = document.get_mut("model_providers") {
        let providers = providers
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("model_providers must be a TOML table"))?;
        changed = providers.remove(GROK_NATIVE_PROVIDER_ID).is_some();
        if providers.is_empty() {
            document.remove("model_providers");
        }
    }
    if changed {
        install_bootstrap_atomically(config_path, document.to_string().as_bytes())?;
    }
    Ok(changed)
}

fn install_shell_environment_guard(document: &mut DocumentMut, env_key: &str) -> Result<()> {
    if !document.contains_key("shell_environment_policy") {
        document["shell_environment_policy"] = Item::Table(Table::new());
    }
    let policy = document["shell_environment_policy"]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("shell_environment_policy must be a TOML table"))?;

    if let Some(set) = policy.get("set") {
        let reintroduces_secret = if let Some(set) = set.as_table() {
            set.iter().any(|(key, _)| key.eq_ignore_ascii_case(env_key))
        } else if let Some(set) = set.as_value().and_then(Value::as_inline_table) {
            set.iter().any(|(key, _)| key.eq_ignore_ascii_case(env_key))
        } else {
            bail!("shell_environment_policy.set must be a TOML table");
        };
        if reintroduces_secret {
            bail!("shell_environment_policy.set reintroduces the provider credential");
        }
    }

    policy["ignore_default_excludes"] = value(false);
    if !policy.contains_key("exclude") {
        policy["exclude"] = Item::Value(Value::Array(Array::new()));
    }
    let excludes = policy["exclude"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("shell_environment_policy.exclude must be an array"))?;
    if excludes.iter().any(|entry| {
        entry
            .as_str()
            .is_some_and(|entry| entry.eq_ignore_ascii_case(env_key))
    }) {
        return Ok(());
    }
    if excludes.iter().any(|entry| entry.as_str().is_none()) {
        bail!("shell_environment_policy.exclude entries must be strings");
    }
    excludes.push(env_key);
    Ok(())
}
