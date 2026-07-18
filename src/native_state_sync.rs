use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, bail};
use filetime::{FileTime, set_file_mtime};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, value};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    },
};

use crate::install_bootstrap_atomically;

const MANIFEST_VERSION: u8 = 1;
const MANIFEST_NAME: &str = "session-import-manifest.json";
const MAX_ROLLOUT_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_SESSION_INDEX_BYTES: u64 = 16 * 1024 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeSessionSyncReceipt {
    pub imported: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped_active: usize,
    pub skipped_invalid: usize,
    pub conflicts: usize,
    pub session_index_entries: usize,
    pub session_index_skipped: bool,
    pub shared_thread_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSharedSessionRollout {
    pub thread_id: String,
    pub daily_path: PathBuf,
    pub isolated_path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SessionImportManifest {
    version: u8,
    files: BTreeMap<String, ImportedSessionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportedSessionRecord {
    thread_id: String,
    source_len: u64,
    source_modified_seconds: i64,
    source_modified_nanos: u32,
    source_sha256: String,
    destination_len: u64,
    destination_sha256: String,
    provider_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StableFileSnapshot {
    len: u64,
    link_count: u32,
    modified_seconds: i64,
    modified_nanos: u32,
    volume_serial: u32,
    file_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StableFileFingerprint {
    snapshot: StableFileSnapshot,
    sha256: String,
}

struct SessionIndexEntry {
    timestamp: Option<(i64, u32)>,
    value: serde_json::Value,
}

pub fn install_isolated_sqlite_home(
    config_path: &Path,
    codex_home: &Path,
    sqlite_home: &Path,
) -> Result<()> {
    if !config_path.is_absolute() || !codex_home.is_absolute() || !sqlite_home.is_absolute() {
        bail!("isolated state paths must be absolute");
    }
    if sqlite_home == codex_home || !sqlite_home.starts_with(codex_home) {
        bail!("isolated sqlite home must be a child of the isolated CODEX_HOME");
    }
    reject_reparse_path(codex_home)?;
    fs::create_dir_all(sqlite_home).with_context(|| {
        format!(
            "failed to create isolated sqlite home {}",
            sqlite_home.display()
        )
    })?;
    reject_reparse_path(sqlite_home)?;
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
    let sqlite = sqlite_home
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("isolated sqlite home is not valid Unicode"))?;
    document["sqlite_home"] = value(sqlite);
    install_bootstrap_atomically(config_path, document.to_string().as_bytes())?;
    Ok(())
}

pub fn native_shared_session_rollouts(
    daily_codex_home: &Path,
    isolated_codex_home: &Path,
) -> Result<Vec<NativeSharedSessionRollout>> {
    if !daily_codex_home.is_absolute() || !isolated_codex_home.is_absolute() {
        bail!("native shared session roots must be absolute");
    }
    let daily = fs::canonicalize(daily_codex_home).with_context(|| {
        format!(
            "failed to resolve daily CODEX_HOME {}",
            daily_codex_home.display()
        )
    })?;
    let isolated = fs::canonicalize(isolated_codex_home).with_context(|| {
        format!(
            "failed to resolve isolated CODEX_HOME {}",
            isolated_codex_home.display()
        )
    })?;
    if daily == isolated {
        bail!("native shared session roots must be disjoint");
    }
    let manifest = load_manifest(&isolated.join("session-import-manifest.json"))?;
    let mut rollouts = Vec::with_capacity(manifest.files.len());
    for (key, record) in manifest.files {
        validate_thread_id(&record.thread_id)?;
        let relative = validated_manifest_relative_path(&key)?;
        if thread_id_from_rollout_path(&relative).as_deref() != Some(record.thread_id.as_str()) {
            bail!("session import manifest thread id does not match its rollout path");
        }
        rollouts.push(NativeSharedSessionRollout {
            thread_id: record.thread_id,
            daily_path: daily.join(&relative),
            isolated_path: isolated.join(relative),
        });
    }
    rollouts.sort_by(|left, right| {
        left.thread_id
            .cmp(&right.thread_id)
            .then_with(|| left.daily_path.cmp(&right.daily_path))
    });
    Ok(rollouts)
}

pub fn sync_native_session_snapshots(
    daily_codex_home: &Path,
    isolated_codex_home: &Path,
    provider_id: &str,
) -> Result<NativeSessionSyncReceipt> {
    validate_provider_id(provider_id)?;
    if !daily_codex_home.is_absolute() || !isolated_codex_home.is_absolute() {
        bail!("native session sync roots must be absolute");
    }
    if daily_codex_home == isolated_codex_home
        || daily_codex_home.starts_with(isolated_codex_home)
        || isolated_codex_home.starts_with(daily_codex_home)
    {
        bail!("daily and isolated CODEX_HOME paths must be disjoint");
    }
    reject_reparse_path(daily_codex_home)?;
    fs::create_dir_all(isolated_codex_home).with_context(|| {
        format!(
            "failed to create isolated CODEX_HOME {}",
            isolated_codex_home.display()
        )
    })?;
    reject_reparse_path(isolated_codex_home)?;

    let manifest_path = isolated_codex_home.join(MANIFEST_NAME);
    let mut manifest = load_manifest(&manifest_path)?;
    let mut receipt = NativeSessionSyncReceipt::default();
    let mut seen_thread_ids = BTreeSet::new();
    let mut candidates = Vec::new();
    for directory in ["sessions", "archived_sessions"] {
        let source_root = daily_codex_home.join(directory);
        if source_root.is_dir() {
            collect_rollout_files(&source_root, &mut candidates)?;
        }
    }
    candidates.sort_by(|left, right| {
        rollout_candidate_priority(daily_codex_home, left)
            .cmp(&rollout_candidate_priority(daily_codex_home, right))
            .then_with(|| left.cmp(right))
    });

    for source in candidates {
        let relative = source
            .strip_prefix(daily_codex_home)
            .context("daily rollout escaped its CODEX_HOME")?;
        let key = relative_path_key(relative)?;
        let thread_id = match thread_id_from_rollout_path(&source) {
            Some(thread_id) => thread_id,
            None => {
                receipt.skipped_invalid += 1;
                continue;
            }
        };
        if seen_thread_ids.contains(&thread_id) {
            receipt.skipped_invalid += 1;
            continue;
        }
        if safe_rollout_metadata(&source).is_err() {
            receipt.skipped_invalid += 1;
            continue;
        }
        seen_thread_ids.insert(thread_id.clone());
        let destination = isolated_codex_home.join(relative);
        let existing_record = manifest.files.get(&key).cloned();
        let prior_thread_record = manifest
            .files
            .iter()
            .find(|(manifest_key, record)| {
                manifest_key.as_str() != key
                    && record.thread_id == thread_id
                    && record.provider_id == provider_id
            })
            .map(|(manifest_key, record)| (manifest_key.clone(), record.clone()));
        if let Some((prior_key, prior_record)) = prior_thread_record.as_ref() {
            let prior_destination =
                isolated_codex_home.join(validated_manifest_relative_path(prior_key)?);
            if prior_destination.exists()
                && !destination_matches_record(&prior_destination, prior_record)?
            {
                receipt.conflicts += 1;
                continue;
            }
        }

        if destination.exists() {
            let source_fingerprint = match stable_rollout_fingerprint(&source) {
                Ok(fingerprint) => fingerprint,
                Err(ImportRolloutError::Active) => {
                    receipt.skipped_active += 1;
                    continue;
                }
                Err(ImportRolloutError::Invalid) => {
                    receipt.skipped_invalid += 1;
                    continue;
                }
                Err(ImportRolloutError::Fatal(error)) => return Err(error),
            };
            let Some(record) = existing_record.as_ref() else {
                match recover_orphaned_import(&source, &destination, &thread_id, provider_id) {
                    Ok(Some(record)) => {
                        let migrated = remove_prior_thread_record(
                            isolated_codex_home,
                            &mut manifest,
                            prior_thread_record.as_ref(),
                        )?;
                        manifest.files.insert(key, record);
                        persist_manifest(&manifest_path, &manifest)?;
                        if migrated {
                            receipt.updated += 1;
                        } else {
                            receipt.unchanged += 1;
                        }
                    }
                    Ok(None) => receipt.conflicts += 1,
                    Err(ImportRolloutError::Active) => receipt.skipped_active += 1,
                    Err(ImportRolloutError::Invalid) => receipt.skipped_invalid += 1,
                    Err(ImportRolloutError::Fatal(error)) => return Err(error),
                }
                continue;
            };
            if record.thread_id != thread_id || record.provider_id != provider_id {
                receipt.conflicts += 1;
                continue;
            }
            let destination_matches = match destination_matches_record(&destination, record) {
                Ok(matches) => matches,
                Err(_) => {
                    receipt.conflicts += 1;
                    continue;
                }
            };
            let source_matches = record.source_len == source_fingerprint.snapshot.len
                && record.source_modified_seconds == source_fingerprint.snapshot.modified_seconds
                && record.source_modified_nanos == source_fingerprint.snapshot.modified_nanos
                && record.source_sha256 == source_fingerprint.sha256;
            if source_matches && destination_matches {
                let migrated = remove_prior_thread_record(
                    isolated_codex_home,
                    &mut manifest,
                    prior_thread_record.as_ref(),
                )?;
                if migrated {
                    persist_manifest(&manifest_path, &manifest)?;
                    receipt.updated += 1;
                } else {
                    receipt.unchanged += 1;
                }
                continue;
            }
            if !destination_matches {
                match recover_orphaned_import(&source, &destination, &thread_id, provider_id) {
                    Ok(Some(recovered_record)) => {
                        remove_prior_thread_record(
                            isolated_codex_home,
                            &mut manifest,
                            prior_thread_record.as_ref(),
                        )?;
                        manifest.files.insert(key, recovered_record);
                        persist_manifest(&manifest_path, &manifest)?;
                        receipt.updated += 1;
                    }
                    Ok(None) => receipt.conflicts += 1,
                    Err(ImportRolloutError::Active) => receipt.skipped_active += 1,
                    Err(ImportRolloutError::Invalid) => receipt.skipped_invalid += 1,
                    Err(ImportRolloutError::Fatal(error)) => return Err(error),
                }
                continue;
            }
        }

        match import_rollout(&source, &destination, &thread_id, provider_id) {
            Ok(record) => {
                let migrated = remove_prior_thread_record(
                    isolated_codex_home,
                    &mut manifest,
                    prior_thread_record.as_ref(),
                )?;
                if existing_record.is_some() || migrated {
                    receipt.updated += 1;
                } else {
                    receipt.imported += 1;
                }
                manifest.files.insert(key, record);
                persist_manifest(&manifest_path, &manifest)?;
            }
            Err(ImportRolloutError::Active) => receipt.skipped_active += 1,
            Err(ImportRolloutError::Invalid) => receipt.skipped_invalid += 1,
            Err(ImportRolloutError::Fatal(error)) => return Err(error),
        }
    }

    match sync_native_session_index(daily_codex_home, isolated_codex_home) {
        Ok(Some(thread_ids)) => {
            receipt.session_index_entries = thread_ids.len();
            receipt.shared_thread_ids = thread_ids;
        }
        Ok(None) => {}
        Err(_) => receipt.session_index_skipped = true,
    }
    manifest.version = MANIFEST_VERSION;
    persist_manifest(&manifest_path, &manifest)?;
    Ok(receipt)
}

fn sync_native_session_index(
    daily_codex_home: &Path,
    isolated_codex_home: &Path,
) -> Result<Option<Vec<String>>> {
    let source_path = daily_codex_home.join("session_index.jsonl");
    let Some(source_entries) = read_session_index(&source_path)? else {
        return Ok(None);
    };
    let destination_path = isolated_codex_home.join("session_index.jsonl");
    let destination_entries = read_session_index(&destination_path)?.unwrap_or_default();
    let mut merged = collapse_session_index_entries(source_entries)?;
    for (id, private_entry) in collapse_session_index_entries(destination_entries)? {
        let private_wins = merged.get(&id).is_none_or(|source_entry| {
            match (source_entry.timestamp, private_entry.timestamp) {
                (Some(source), Some(private)) => private >= source,
                (None, Some(_)) | (None, None) => true,
                (Some(_), None) => false,
            }
        });
        if private_wins {
            merged.insert(id, private_entry);
        }
    }
    let thread_ids = merged.keys().cloned().collect::<Vec<_>>();

    let mut rendered = Vec::new();
    for entry in merged.into_values() {
        serde_json::to_writer(&mut rendered, &entry.value)?;
        rendered.push(b'\n');
    }
    match fs::read(&destination_path) {
        Ok(existing) if existing == rendered => {}
        Ok(_) => {
            install_bootstrap_atomically(&destination_path, &rendered)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            install_bootstrap_atomically(&destination_path, &rendered)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(Some(thread_ids))
}

fn read_session_index(path: &Path) -> Result<Option<Vec<serde_json::Value>>> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let (first_snapshot, bytes) = read_session_index_once(path)?;
    let (second_snapshot, second_bytes) = read_session_index_once(path)?;
    if first_snapshot != second_snapshot || bytes != second_bytes {
        bail!("native session index changed during import");
    }
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bail!("native session index has a partial final line");
    }

    let mut entries = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_slice(line).context("native session index contains invalid JSON")?;
        session_index_identity(&value)?;
        entries.push(value);
    }
    Ok(Some(entries))
}

fn read_session_index_once(path: &Path) -> Result<(StableFileSnapshot, Vec<u8>)> {
    let mut file = open_shared_native_state_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > MAX_SESSION_INDEX_BYTES
    {
        bail!("native session index has an invalid file shape or size");
    }
    let initial = file_snapshot(&file)?;
    if initial.link_count != 1 {
        bail!("native session index must not be a hard link");
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_SESSION_INDEX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SESSION_INDEX_BYTES {
        bail!("native session index exceeds its size limit");
    }
    if file_snapshot(&file)? != initial {
        bail!("native session index changed during import");
    }
    Ok((initial, bytes))
}

fn collapse_session_index_entries(
    entries: Vec<serde_json::Value>,
) -> Result<BTreeMap<String, SessionIndexEntry>> {
    let mut collapsed = BTreeMap::new();
    for value in entries {
        let (id, updated_at) = session_index_identity(&value)?;
        collapsed.insert(
            id,
            SessionIndexEntry {
                timestamp: parse_rfc3339_timestamp(&updated_at),
                value,
            },
        );
    }
    Ok(collapsed)
}

fn parse_rfc3339_timestamp(value: &str) -> Option<(i64, u32)> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year = parse_decimal(&bytes[0..4])? as i64;
    let month = parse_decimal(&bytes[5..7])?;
    let day = parse_decimal(&bytes[8..10])?;
    let hour = parse_decimal(&bytes[11..13])?;
    let minute = parse_decimal(&bytes[14..16])?;
    let second = parse_decimal(&bytes[17..19])?;
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let mut cursor = 19;
    let mut nanos = 0_u32;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let fraction_start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        let digits = cursor - fraction_start;
        if digits == 0 || digits > 9 {
            return None;
        }
        nanos = parse_decimal(&bytes[fraction_start..cursor])?
            .checked_mul(10_u32.pow((9 - digits) as u32))?;
    }

    let offset_seconds = match bytes.get(cursor) {
        Some(b'Z' | b'z') if cursor + 1 == bytes.len() => 0_i64,
        Some(sign @ (b'+' | b'-')) if cursor + 6 == bytes.len() => {
            if bytes[cursor + 3] != b':' {
                return None;
            }
            let offset_hour = parse_decimal(&bytes[cursor + 1..cursor + 3])?;
            let offset_minute = parse_decimal(&bytes[cursor + 4..cursor + 6])?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            let magnitude = i64::from(offset_hour * 3600 + offset_minute * 60);
            if *sign == b'+' { magnitude } else { -magnitude }
        }
        _ => return None,
    };
    let seconds = days_from_civil(year, month, day)
        .checked_mul(86_400)?
        .checked_add(i64::from(hour * 3600 + minute * 60 + second))?
        .checked_sub(offset_seconds)?;
    Some((seconds, nanos))
}

fn parse_decimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        if byte.is_ascii_digit() {
            Some(value * 10 + u32::from(*byte - b'0'))
        } else {
            None
        }
    })
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn session_index_identity(value: &serde_json::Value) -> Result<(String, String)> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("native session index entry must be an object"))?;
    let id = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("native session index entry has no id"))?;
    validate_thread_id(id)?;
    let name = object
        .get("thread_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("native session index entry has no thread name"))?;
    validate_index_text(name, "thread name", 4096)?;
    let updated_at = object
        .get("updated_at")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("native session index entry has no updated_at"))?;
    validate_index_text(updated_at, "updated_at", 128)?;
    Ok((id.to_ascii_lowercase(), updated_at.to_owned()))
}

fn validate_index_text(value: &str, label: &str, max_len: usize) -> Result<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.chars().any(|character| character.is_control())
    {
        bail!("native session index contains an invalid {label}");
    }
    Ok(())
}

enum ImportRolloutError {
    Active,
    Invalid,
    Fatal(anyhow::Error),
}

fn rollout_candidate_priority(daily_codex_home: &Path, path: &Path) -> u8 {
    path.strip_prefix(daily_codex_home)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .map_or(2, |directory| match directory {
            "sessions" => 0,
            "archived_sessions" => 1,
            _ => 2,
        })
}

fn open_shared_native_state_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn stable_rollout_fingerprint(
    path: &Path,
) -> std::result::Result<StableFileFingerprint, ImportRolloutError> {
    let file = match open_shared_native_state_file(path) {
        Ok(file) => file,
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.kind() == std::io::ErrorKind::NotFound
                || matches!(error.raw_os_error(), Some(32 | 33)) =>
        {
            return Err(ImportRolloutError::Active);
        }
        Err(error) => {
            return Err(ImportRolloutError::Fatal(
                anyhow::Error::new(error).context("failed to open daily rollout"),
            ));
        }
    };
    let metadata = file
        .metadata()
        .map_err(|error| ImportRolloutError::Fatal(error.into()))?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() == 0
        || metadata.len() > MAX_ROLLOUT_BYTES
    {
        return Err(ImportRolloutError::Invalid);
    }
    let initial = file_snapshot(&file).map_err(ImportRolloutError::Fatal)?;
    if initial.link_count != 1 {
        return Err(ImportRolloutError::Invalid);
    }
    let mut reader = BufReader::new(file);
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| ImportRolloutError::Fatal(error.into()))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    let ending = file_snapshot(reader.get_ref()).map_err(ImportRolloutError::Fatal)?;
    if ending != initial {
        return Err(ImportRolloutError::Active);
    }
    Ok(StableFileFingerprint {
        snapshot: initial,
        sha256: format!("{:x}", hash.finalize()),
    })
}

fn file_snapshot(file: &File) -> Result<StableFileSnapshot> {
    let metadata = file.metadata()?;
    let modified = FileTime::from_last_modification_time(&metadata);
    let mut information: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut information) } == 0
    {
        return Err(std::io::Error::last_os_error())
            .context("failed to resolve the native state file identity");
    }
    Ok(StableFileSnapshot {
        len: metadata.len(),
        link_count: information.nNumberOfLinks,
        modified_seconds: modified.unix_seconds(),
        modified_nanos: modified.nanoseconds(),
        volume_serial: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

fn recover_orphaned_import(
    source: &Path,
    destination: &Path,
    expected_thread_id: &str,
    provider_id: &str,
) -> std::result::Result<Option<ImportedSessionRecord>, ImportRolloutError> {
    let recovery_destination = unique_temp_path(destination);
    let record = import_rollout(
        source,
        &recovery_destination,
        expected_thread_id,
        provider_id,
    )?;
    let matches =
        destination_matches_record(destination, &record).map_err(ImportRolloutError::Fatal)?;
    fs::remove_file(&recovery_destination).map_err(|error| {
        ImportRolloutError::Fatal(
            anyhow::Error::new(error).context("failed to remove orphan-recovery snapshot"),
        )
    })?;
    if !matches {
        return Ok(None);
    }
    set_file_mtime(
        destination,
        FileTime::from_unix_time(record.source_modified_seconds, record.source_modified_nanos),
    )
    .map_err(|error| ImportRolloutError::Fatal(error.into()))?;
    Ok(Some(record))
}

fn remove_prior_thread_record(
    isolated_codex_home: &Path,
    manifest: &mut SessionImportManifest,
    prior: Option<&(String, ImportedSessionRecord)>,
) -> Result<bool> {
    let Some((key, record)) = prior else {
        return Ok(false);
    };
    let relative = validated_manifest_relative_path(key)?;
    let destination = isolated_codex_home.join(relative);
    if destination.exists() {
        if !destination_matches_record(&destination, record)? {
            bail!("prior private rollout changed during path migration");
        }
        fs::remove_file(&destination).with_context(|| {
            format!(
                "failed to remove prior private rollout {}",
                destination.display()
            )
        })?;
    }
    manifest.files.remove(key);
    Ok(true)
}

fn destination_matches_record(path: &Path, record: &ImportedSessionRecord) -> Result<bool> {
    let metadata = safe_rollout_metadata(path)?;
    Ok(metadata.len() == record.destination_len && sha256_file(path)? == record.destination_sha256)
}

fn persist_manifest(path: &Path, manifest: &SessionImportManifest) -> Result<()> {
    let content = serde_json::to_vec_pretty(manifest)?;
    install_bootstrap_atomically(path, &content)?;
    Ok(())
}

fn validated_manifest_relative_path(value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!("session import manifest contains an invalid relative path");
    }
    Ok(path)
}

fn import_rollout(
    source: &Path,
    destination: &Path,
    expected_thread_id: &str,
    provider_id: &str,
) -> std::result::Result<ImportedSessionRecord, ImportRolloutError> {
    let file = match open_shared_native_state_file(source) {
        Ok(file) => file,
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.kind() == std::io::ErrorKind::NotFound
                || matches!(error.raw_os_error(), Some(32 | 33)) =>
        {
            return Err(ImportRolloutError::Active);
        }
        Err(error) => {
            return Err(ImportRolloutError::Fatal(
                anyhow::Error::new(error).context("failed to open daily rollout"),
            ));
        }
    };
    let metadata = file
        .metadata()
        .map_err(|error| ImportRolloutError::Fatal(error.into()))?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() == 0
        || metadata.len() > MAX_ROLLOUT_BYTES
    {
        return Err(ImportRolloutError::Invalid);
    }
    let source_time = FileTime::from_last_modification_time(&metadata);
    let initial_snapshot = file_snapshot(&file).map_err(ImportRolloutError::Fatal)?;
    if initial_snapshot.link_count != 1 {
        return Err(ImportRolloutError::Invalid);
    }
    let parent = destination.parent().ok_or_else(|| {
        ImportRolloutError::Fatal(anyhow::anyhow!("rollout destination has no parent"))
    })?;
    fs::create_dir_all(parent).map_err(|error| ImportRolloutError::Fatal(error.into()))?;
    reject_reparse_path(parent).map_err(ImportRolloutError::Fatal)?;
    let temp = unique_temp_path(destination);
    let imported = (|| -> Result<ImportedSessionRecord> {
        let mut reader = BufReader::new(file);
        let output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        let mut writer = BufWriter::new(output);
        let mut source_hash = Sha256::new();
        let mut destination_hash = Sha256::new();
        let mut line = Vec::new();
        let mut line_number = 0_usize;
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            line_number += 1;
            source_hash.update(&line);
            if !line.ends_with(b"\n") {
                bail!("daily rollout has a partial final line");
            }
            let json_bytes = line
                .strip_suffix(b"\n")
                .unwrap_or(&line)
                .strip_suffix(b"\r")
                .unwrap_or_else(|| line.strip_suffix(b"\n").unwrap_or(&line));
            if json_bytes.is_empty() {
                writer.write_all(&line)?;
                destination_hash.update(&line);
                continue;
            }
            let mut value: serde_json::Value = serde_json::from_slice(json_bytes)
                .context("daily rollout contains invalid JSON")?;
            if line_number == 1 {
                if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta")
                    || value
                        .get("payload")
                        .and_then(|payload| payload.get("id"))
                        .and_then(serde_json::Value::as_str)
                        != Some(expected_thread_id)
                {
                    bail!("daily rollout canonical session metadata is invalid");
                }
                let payload = value
                    .get_mut("payload")
                    .and_then(serde_json::Value::as_object_mut)
                    .ok_or_else(|| anyhow::anyhow!("daily rollout session payload is invalid"))?;
                payload.insert(
                    "model_provider".into(),
                    serde_json::Value::String(provider_id.into()),
                );
                let mut rendered = serde_json::to_vec(&value)?;
                rendered.push(b'\n');
                writer.write_all(&rendered)?;
                destination_hash.update(&rendered);
            } else {
                writer.write_all(&line)?;
                destination_hash.update(&line);
            }
        }
        if line_number == 0 {
            bail!("daily rollout is empty");
        }
        writer.flush()?;
        let output = writer.into_inner()?;
        output.sync_all()?;
        let ending_snapshot = file_snapshot(reader.get_ref())?;
        if ending_snapshot != initial_snapshot {
            bail!("daily rollout changed during import");
        }
        let source_sha256 = format!("{:x}", source_hash.finalize());
        let verification = match stable_rollout_fingerprint(source) {
            Ok(fingerprint) => fingerprint,
            Err(ImportRolloutError::Active) => {
                bail!("daily rollout changed during import")
            }
            Err(ImportRolloutError::Invalid) => {
                bail!("daily rollout became invalid during import")
            }
            Err(ImportRolloutError::Fatal(error)) => return Err(error),
        };
        if verification.snapshot != initial_snapshot || verification.sha256 != source_sha256 {
            bail!("daily rollout changed during import");
        }
        atomic_replace(&temp, destination)?;
        set_file_mtime(destination, source_time)?;
        let destination_len = fs::metadata(destination)?.len();
        Ok(ImportedSessionRecord {
            thread_id: expected_thread_id.into(),
            source_len: initial_snapshot.len,
            source_modified_seconds: initial_snapshot.modified_seconds,
            source_modified_nanos: initial_snapshot.modified_nanos,
            source_sha256,
            destination_len,
            destination_sha256: format!("{:x}", destination_hash.finalize()),
            provider_id: provider_id.into(),
        })
    })();
    if imported.is_err() {
        let _ = fs::remove_file(&temp);
    }
    imported.map_err(|error| {
        let message = error.to_string();
        if message.contains("changed during import") {
            ImportRolloutError::Active
        } else if message.contains("daily rollout") {
            ImportRolloutError::Invalid
        } else {
            ImportRolloutError::Fatal(error)
        }
    })
}

fn collect_rollout_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    reject_reparse_path(directory)?;
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to enumerate {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            continue;
        }
        if metadata.is_dir() {
            collect_rollout_files(&path, output)?;
        } else if metadata.is_file() && path.extension().and_then(OsStr::to_str) == Some("jsonl") {
            output.push(path);
        }
    }
    Ok(())
}

fn safe_rollout_metadata(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() == 0
        || metadata.len() > MAX_ROLLOUT_BYTES
    {
        bail!("rollout has an invalid file shape");
    }
    Ok(metadata)
}

fn load_manifest(path: &Path) -> Result<SessionImportManifest> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionImportManifest {
                version: MANIFEST_VERSION,
                files: BTreeMap::new(),
            });
        }
        Err(error) => return Err(error.into()),
    };
    let manifest: SessionImportManifest =
        serde_json::from_slice(&bytes).context("isolated session import manifest is invalid")?;
    if manifest.version != MANIFEST_VERSION {
        bail!("isolated session import manifest version is unsupported");
    }
    Ok(manifest)
}

fn thread_id_from_rollout_path(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let id = stem.rsplit('-').collect::<Vec<_>>();
    if id.len() < 5 {
        return None;
    }
    let candidate = stem.get(stem.len().checked_sub(36)?..)?;
    validate_thread_id(candidate).ok()?;
    Some(candidate.to_ascii_lowercase())
}

fn validate_thread_id(candidate: &str) -> Result<()> {
    let bytes = candidate.as_bytes();
    if bytes.len() != 36
        || ![8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| ![8, 13, 18, 23].contains(&index) && !byte.is_ascii_hexdigit())
    {
        bail!("thread id is invalid");
    }
    Ok(())
}

fn validate_provider_id(provider_id: &str) -> Result<()> {
    if provider_id.is_empty()
        || provider_id.len() > 128
        || provider_id.trim() != provider_id
        || !provider_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        bail!("session import provider ID is invalid");
    }
    Ok(())
}

fn relative_path_key(path: &Path) -> Result<String> {
    let parts = path
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow::anyhow!("rollout relative path is not valid Unicode"))?;
    Ok(parts.join("/"))
}

fn reject_reparse_path(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                bail!("native state path contains a reparse point");
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn unique_temp_path(target: &Path) -> PathBuf {
    let suffix = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("rollout.jsonl");
    target.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), suffix))
}

fn atomic_replace(source: &Path, target: &Path) -> Result<()> {
    let source = wide_null(source.as_os_str());
    let target = wide_null(target.as_os_str());
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error()).context("failed to publish session snapshot");
    }
    Ok(())
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
