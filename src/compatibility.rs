use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use crate::{AgentMode, HostAdapterKind};

#[derive(Debug, Clone, Default)]
pub struct CompatibilityPolicy {
    allowed_versions: BTreeMap<HostAdapterKind, BTreeSet<String>>,
}

impl CompatibilityPolicy {
    pub fn allow(mut self, adapter: HostAdapterKind, version: &str) -> Result<Self> {
        let version = version.trim();
        if version.is_empty() {
            bail!("host version cannot be blank");
        }
        if version.len() > 256 {
            bail!("host version cannot exceed 256 bytes");
        }
        self.allowed_versions
            .entry(adapter)
            .or_default()
            .insert(version.to_owned());
        Ok(self)
    }

    pub fn evaluate(
        &self,
        adapter: HostAdapterKind,
        version: Option<&str>,
        requested: AgentMode,
    ) -> CompatibilityDecision {
        let verified = version
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some_and(|version| {
                self.allowed_versions
                    .get(&adapter)
                    .is_some_and(|versions| versions.contains(version))
            });
        if verified {
            CompatibilityDecision::Enabled(requested)
        } else {
            CompatibilityDecision::NativeOnly {
                requested,
                reason: "unverified_host_version".into(),
            }
        }
    }
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
