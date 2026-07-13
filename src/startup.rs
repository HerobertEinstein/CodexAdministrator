use std::path::Path;

use anyhow::Result;

use crate::{
    BootstrapConfig, CompatibilityDecision, CompatibilityPolicy, HostAdapterKind, HostIdentity,
    codex_plus_bootstrap_path, enable_codex_plus_bootstrap, install_bootstrap_atomically,
    remove_codex_plus_bootstrap, render_bootstrap,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPlusPreparation {
    pub bootstrap_path: std::path::PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexPlusStartupOutcome {
    pub decision: CompatibilityDecision,
    pub bootstrap: Option<CodexPlusPreparation>,
    pub isolation_error: Option<String>,
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

pub fn prepare_codex_plus_host_guarded(
    appdata: &Path,
    bootstrap_config: &BootstrapConfig,
    identity: Option<&HostIdentity>,
    policy: &CompatibilityPolicy,
) -> CodexPlusStartupOutcome {
    let identity_sha256 = identity
        .filter(|identity| identity.adapter == HostAdapterKind::CodexPlusPlus)
        .map(|identity| identity.sha256.as_str());
    let decision = policy.evaluate(HostAdapterKind::CodexPlusPlus, identity_sha256);

    if decision.injection_enabled() {
        match prepare_codex_plus_host(appdata, bootstrap_config) {
            Ok(bootstrap) => {
                return CodexPlusStartupOutcome {
                    decision,
                    bootstrap: Some(bootstrap),
                    isolation_error: None,
                };
            }
            Err(error) => {
                let cleanup_error = remove_codex_plus_bootstrap(appdata).err();
                let error = match cleanup_error {
                    Some(cleanup_error) => {
                        format!(
                            "bootstrap preparation failed: {error}; cleanup failed: {cleanup_error}"
                        )
                    }
                    None => format!("bootstrap preparation failed: {error}"),
                };
                return CodexPlusStartupOutcome {
                    decision: CompatibilityDecision::NativeOnly {
                        reason: "bootstrap_prepare_failed".into(),
                    },
                    bootstrap: None,
                    isolation_error: Some(error),
                };
            }
        }
    }

    let isolation_error = remove_codex_plus_bootstrap(appdata)
        .err()
        .map(|error| format!("failed to remove stale project bootstrap: {error}"));
    CodexPlusStartupOutcome {
        decision,
        bootstrap: None,
        isolation_error,
    }
}
