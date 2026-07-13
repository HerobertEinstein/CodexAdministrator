use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use http::Uri;
use sha2::Digest;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::install_bootstrap_atomically;

pub const GROK_NATIVE_PROVIDER_ID: &str = "grok_native";

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
        if !uri.path().trim_end_matches('/').ends_with("/v1") {
            bail!("Grok provider base URL must end in /v1");
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
    table["base_url"] = value(provider.base_url.as_str());
    table["env_key"] = value(provider.env_key.as_str());
    table["wire_api"] = value("responses");
    table["requires_openai_auth"] = value(false);
    table["supports_websockets"] = value(provider.supports_websockets);

    let rendered = document.to_string();
    let updated = rendered != existing;
    let sha256 = if updated {
        install_bootstrap_atomically(config_path, rendered.as_bytes())?
    } else {
        format!("{:x}", sha2::Sha256::digest(rendered.as_bytes()))
    };
    Ok(NativeProviderInstallReceipt { updated, sha256 })
}
