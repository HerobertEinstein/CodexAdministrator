use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::install_bootstrap_atomically;

const CONTINUITY_MANIFEST_VERSION: u8 = 1;
const MAX_CONTINUITY_THREADS: usize = 4096;
const MAX_CONTINUITY_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeTurnStatus {
    Completed,
    Interrupted,
    Failed,
    InProgress,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTurnCheckpoint {
    pub id: String,
    pub fingerprint: String,
    pub status: NativeTurnStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSessionHead {
    pub thread_id: String,
    pub model_provider: String,
    /// Turns are ordered newest first.
    pub turns: Vec<NativeTurnCheckpoint>,
    pub history_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeSessionRelation {
    Equal,
    DailyAhead,
    IsolatedAhead,
    Diverged,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeSessionContinuity {
    pub thread_id: String,
    pub relation: NativeSessionRelation,
    pub common_turn_id: Option<String>,
    pub daily_head_id: Option<String>,
    pub isolated_head_id: Option<String>,
}

pub trait NativeSessionHeadStore {
    fn read_session_head(&mut self, thread_id: &str) -> Result<NativeSessionHead>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeSessionContinuityReceipt {
    pub threads: usize,
    pub equal: usize,
    pub daily_ahead: usize,
    pub isolated_ahead: usize,
    pub diverged: usize,
    pub unknown: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeSessionContinuityRecord {
    daily: NativeSessionHead,
    isolated: NativeSessionHead,
    continuity: NativeSessionContinuity,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NativeSessionContinuityManifest {
    version: u8,
    records: BTreeMap<String, NativeSessionContinuityRecord>,
}

pub fn observe_native_session_continuity<D, I, T>(
    daily: &mut D,
    isolated: &mut I,
    thread_ids: T,
    manifest_path: &Path,
) -> Result<NativeSessionContinuityReceipt>
where
    D: NativeSessionHeadStore,
    I: NativeSessionHeadStore,
    T: IntoIterator,
    T::Item: AsRef<str>,
{
    let mut manifest = load_manifest(manifest_path)?;
    let mut receipt = NativeSessionContinuityReceipt::default();
    let mut observed = BTreeSet::new();
    for thread_id in thread_ids {
        let thread_id = thread_id.as_ref();
        if thread_id.trim().is_empty() || !observed.insert(thread_id.to_owned()) {
            bail!("session continuity thread ids must be unique and non-empty");
        }
        if observed.len() > MAX_CONTINUITY_THREADS {
            bail!("session continuity observation exceeds its thread limit");
        }
        let daily_head = daily.read_session_head(thread_id)?;
        let isolated_head = isolated.read_session_head(thread_id)?;
        let continuity = compare_native_session_heads(&daily_head, &isolated_head)?;
        receipt.threads += 1;
        match continuity.relation {
            NativeSessionRelation::Equal => receipt.equal += 1,
            NativeSessionRelation::DailyAhead => receipt.daily_ahead += 1,
            NativeSessionRelation::IsolatedAhead => receipt.isolated_ahead += 1,
            NativeSessionRelation::Diverged => receipt.diverged += 1,
            NativeSessionRelation::Unknown => receipt.unknown += 1,
        }
        manifest.records.insert(
            thread_id.to_owned(),
            NativeSessionContinuityRecord {
                daily: daily_head,
                isolated: isolated_head,
                continuity,
            },
        );
    }
    manifest.version = CONTINUITY_MANIFEST_VERSION;
    persist_manifest(manifest_path, &manifest)?;
    Ok(receipt)
}

pub fn compare_native_session_heads(
    daily: &NativeSessionHead,
    isolated: &NativeSessionHead,
) -> Result<NativeSessionContinuity> {
    validate_head(daily)?;
    validate_head(isolated)?;
    if daily.thread_id != isolated.thread_id {
        bail!("session heads must belong to the same logical thread");
    }

    let daily_latest = daily.turns.first();
    let isolated_latest = isolated.turns.first();
    let common = daily
        .turns
        .iter()
        .find(|turn| isolated.turns.iter().any(|other| exact_turn(turn, other)));

    let relation = match (daily_latest, isolated_latest) {
        (None, None) => NativeSessionRelation::Equal,
        (Some(_), None) if isolated.history_complete => NativeSessionRelation::DailyAhead,
        (None, Some(_)) if daily.history_complete => NativeSessionRelation::IsolatedAhead,
        (Some(_), None) | (None, Some(_)) => NativeSessionRelation::Unknown,
        (Some(daily_latest), Some(isolated_latest))
            if exact_turn(daily_latest, isolated_latest) =>
        {
            NativeSessionRelation::Equal
        }
        (_, Some(isolated_latest))
            if daily
                .turns
                .iter()
                .any(|turn| exact_turn(turn, isolated_latest)) =>
        {
            NativeSessionRelation::DailyAhead
        }
        (Some(daily_latest), _)
            if isolated
                .turns
                .iter()
                .any(|turn| exact_turn(turn, daily_latest)) =>
        {
            NativeSessionRelation::IsolatedAhead
        }
        _ if common.is_none() && (!daily.history_complete || !isolated.history_complete) => {
            NativeSessionRelation::Unknown
        }
        _ => NativeSessionRelation::Diverged,
    };

    Ok(NativeSessionContinuity {
        thread_id: daily.thread_id.clone(),
        relation,
        common_turn_id: common.map(|turn| turn.id.clone()),
        daily_head_id: daily_latest.map(|turn| turn.id.clone()),
        isolated_head_id: isolated_latest.map(|turn| turn.id.clone()),
    })
}

fn exact_turn(left: &NativeTurnCheckpoint, right: &NativeTurnCheckpoint) -> bool {
    left.id == right.id && left.fingerprint == right.fingerprint
}

fn validate_head(head: &NativeSessionHead) -> Result<()> {
    if head.thread_id.trim().is_empty() {
        bail!("session head thread id must not be empty");
    }
    if head.model_provider.trim().is_empty() {
        bail!("session head model provider must not be empty");
    }
    let mut ids = BTreeSet::new();
    for turn in &head.turns {
        if turn.id.trim().is_empty() || turn.fingerprint.trim().is_empty() {
            bail!("session turn checkpoint fields must not be empty");
        }
        if !ids.insert(turn.id.as_str()) {
            bail!("session head contains a duplicate turn id");
        }
    }
    Ok(())
}

fn load_manifest(path: &Path) -> Result<NativeSessionContinuityManifest> {
    if !path.exists() {
        return Ok(NativeSessionContinuityManifest {
            version: CONTINUITY_MANIFEST_VERSION,
            records: BTreeMap::new(),
        });
    }
    let metadata = fs::metadata(path).with_context(|| {
        format!(
            "failed to inspect session continuity manifest {}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.len() > MAX_CONTINUITY_MANIFEST_BYTES {
        bail!("session continuity manifest is not a bounded regular file");
    }
    let manifest: NativeSessionContinuityManifest =
        serde_json::from_slice(&fs::read(path).with_context(|| {
            format!(
                "failed to read session continuity manifest {}",
                path.display()
            )
        })?)
        .context("session continuity manifest is invalid")?;
    if manifest.version != CONTINUITY_MANIFEST_VERSION {
        bail!("session continuity manifest version is unsupported");
    }
    if manifest.records.len() > MAX_CONTINUITY_THREADS {
        bail!("session continuity manifest exceeds its thread limit");
    }
    Ok(manifest)
}

fn persist_manifest(path: &Path, manifest: &NativeSessionContinuityManifest) -> Result<()> {
    let content = serde_json::to_vec_pretty(manifest)?;
    if content.len() as u64 > MAX_CONTINUITY_MANIFEST_BYTES {
        bail!("session continuity manifest exceeds its size limit");
    }
    install_bootstrap_atomically(path, &content)?;
    Ok(())
}
