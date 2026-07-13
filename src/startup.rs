use std::{fmt::Write, path::Path};

use anyhow::Result;
use rand::RngCore;

use crate::{
    BootstrapConfig, codex_plus_bootstrap_path, enable_codex_plus_bootstrap,
    install_bootstrap_atomically, render_bootstrap,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPlusPreparation {
    pub bootstrap_path: std::path::PathBuf,
    pub sha256: String,
}

pub fn generate_capability() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let mut capability = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut capability, "{byte:02x}").expect("writing to a String cannot fail");
    }
    capability
}

pub fn prepare_codex_plus_host(
    appdata: &Path,
    bootstrap_config: &BootstrapConfig,
) -> Result<CodexPlusPreparation> {
    let script = render_bootstrap(bootstrap_config)?;
    let bootstrap_path = codex_plus_bootstrap_path(appdata);
    let sha256 = install_bootstrap_atomically(&bootstrap_path, script.as_bytes())?;
    enable_codex_plus_bootstrap(&appdata.join("Codex++").join("user_scripts.json"))?;
    Ok(CodexPlusPreparation {
        bootstrap_path,
        sha256,
    })
}
