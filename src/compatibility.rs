use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{AgentMode, HostAdapterKind};

#[derive(Debug, Clone, Default)]
pub struct CompatibilityPolicy {
    allowed_host_sha256: BTreeMap<HostAdapterKind, BTreeSet<String>>,
}

impl CompatibilityPolicy {
    pub fn allow_host_sha256(mut self, adapter: HostAdapterKind, sha256: &str) -> Result<Self> {
        let sha256 = normalize_sha256(sha256)?;
        self.allowed_host_sha256
            .entry(adapter)
            .or_default()
            .insert(sha256);
        Ok(self)
    }

    pub fn evaluate(
        &self,
        adapter: HostAdapterKind,
        sha256: Option<&str>,
        requested: AgentMode,
    ) -> CompatibilityDecision {
        let verified = sha256
            .and_then(|value| normalize_sha256(value).ok())
            .is_some_and(|sha256| {
                self.allowed_host_sha256
                    .get(&adapter)
                    .is_some_and(|hashes| hashes.contains(&sha256))
            });
        if verified {
            CompatibilityDecision::Enabled(requested)
        } else {
            CompatibilityDecision::NativeOnly {
                requested,
                reason: "unverified_host_identity".into(),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostIdentity {
    pub adapter: HostAdapterKind,
    pub sha256: String,
}

impl HostIdentity {
    pub fn from_executable(adapter: HostAdapterKind, executable: &Path) -> Result<Self> {
        let file = File::open(executable)
            .with_context(|| format!("failed to open host executable {}", executable.display()))?;
        let mut reader = BufReader::new(file);
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer).with_context(|| {
                format!("failed to read host executable {}", executable.display())
            })?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        let digest = digest.finalize();
        Ok(Self {
            adapter,
            sha256: format!("{digest:x}"),
        })
    }

    pub fn matches_executable(&self, executable: &Path) -> Result<bool> {
        let observed = Self::from_executable(self.adapter, executable)?;
        Ok(self.sha256 == observed.sha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityManifest {
    schema_version: u8,
    hosts: Vec<CompatibilityManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompatibilityManifestEntry {
    adapter: HostAdapterKind,
    sha256: String,
    project_version: String,
    bootstrap_version: u8,
    evidence_sha256: String,
}

impl CompatibilityManifest {
    pub fn shipped() -> Result<Self> {
        Self::from_json(include_bytes!("../compatibility.json"))
    }

    pub fn from_json(content: &[u8]) -> Result<Self> {
        let manifest: Self =
            serde_json::from_slice(content).context("invalid compatibility manifest")?;
        if manifest.schema_version != 1 {
            bail!(
                "unsupported compatibility manifest schema version {}",
                manifest.schema_version
            );
        }
        if manifest.hosts.len() > 1024 {
            bail!("compatibility manifest contains too many host identities");
        }
        Ok(manifest)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read(path)
            .with_context(|| format!("failed to read compatibility manifest {}", path.display()))?;
        Self::from_json(&content)
    }

    pub fn into_policy(self) -> Result<CompatibilityPolicy> {
        self.hosts
            .into_iter()
            .try_fold(CompatibilityPolicy::default(), |policy, host| {
                if host.project_version != env!("CARGO_PKG_VERSION") {
                    bail!(
                        "compatibility entry targets project version {}, expected {}",
                        host.project_version,
                        env!("CARGO_PKG_VERSION")
                    );
                }
                if host.bootstrap_version != 1 {
                    bail!(
                        "compatibility entry targets unsupported bootstrap version {}",
                        host.bootstrap_version
                    );
                }
                normalize_sha256(&host.evidence_sha256)
                    .context("compatibility entry has an invalid E2E evidence digest")?;
                policy.allow_host_sha256(host.adapter, &host.sha256)
            })
    }
}

fn normalize_sha256(value: &str) -> Result<String> {
    let value = value.trim();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("host identity must be a 64-character SHA-256 value");
    }
    Ok(value.to_ascii_lowercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityDecision {
    Enabled(AgentMode),
    NativeOnly {
        requested: AgentMode,
        reason: String,
    },
}

impl CompatibilityDecision {
    pub const fn effective_mode(&self) -> AgentMode {
        match self {
            Self::Enabled(mode) => *mode,
            Self::NativeOnly { .. } => AgentMode::NativeGptMain,
        }
    }

    pub const fn injection_enabled(&self) -> bool {
        matches!(self, Self::Enabled(AgentMode::GrokInjectedMain))
    }
}
