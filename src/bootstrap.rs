use anyhow::{Result, bail};
use serde::Serialize;

const BOOTSTRAP_TEMPLATE: &str = include_str!("../assets/bootstrap.js");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapConfig {
    pub port: u16,
    pub capability: String,
}

#[derive(Serialize)]
struct SerializedBootstrapConfig<'a> {
    version: u8,
    base_url: String,
    capability: &'a str,
}

pub fn render_bootstrap(config: &BootstrapConfig) -> Result<String> {
    if config.port == 0 {
        bail!("bootstrap port cannot be zero");
    }
    let capability = config.capability.trim();
    if capability.is_empty() {
        bail!("bootstrap capability cannot be blank");
    }
    if capability.len() > 512 {
        bail!("bootstrap capability is too long");
    }

    let serialized = serde_json::to_string(&SerializedBootstrapConfig {
        version: 1,
        base_url: format!("http://127.0.0.1:{}", config.port),
        capability,
    })?;

    Ok(BOOTSTRAP_TEMPLATE.replace("/*__CODEX_ADMINISTRATOR_CONFIG__*/", &serialized))
}
