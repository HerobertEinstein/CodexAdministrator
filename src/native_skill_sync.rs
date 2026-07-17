use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Read,
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle,
    },
};

use crate::install_bootstrap_atomically;

const MANIFEST_NAME: &str = "skill-projection-manifest.json";
const MANIFEST_VERSION: u8 = 1;
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SKILL_FILE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SkillProjectionManifest {
    version: u8,
    files: BTreeMap<String, SkillProjectionRecord>,
    conflicts: BTreeMap<String, SkillProjectionConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillProjectionRecord {
    source_sha256: String,
    destination_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillProjectionConflict {
    reason: String,
    source_sha256: Option<String>,
    destination_sha256: Option<String>,
}

enum DestinationState {
    HardLink(String),
    Missing,
    NotRegular,
    ReparsePoint,
    Regular(String),
}

enum SourceFileState {
    Content(Vec<u8>),
    HardLink,
    NotRegular,
    ReparsePoint,
    TooLarge,
    Unstable,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct StableFileSnapshot {
    attributes: u32,
    file_index: u64,
    last_write_high: u32,
    last_write_low: u32,
    len: u64,
    link_count: u32,
    volume_serial: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeSkillSyncReceipt {
    pub projected: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub conflicts: usize,
    pub skipped: usize,
}

pub fn sync_native_skills(
    daily_codex_home: &Path,
    isolated_codex_home: &Path,
) -> Result<NativeSkillSyncReceipt> {
    if !daily_codex_home.is_absolute() || !isolated_codex_home.is_absolute() {
        bail!("native Skill sync roots must be absolute");
    }
    if daily_codex_home == isolated_codex_home
        || daily_codex_home.starts_with(isolated_codex_home)
        || isolated_codex_home.starts_with(daily_codex_home)
    {
        bail!("daily and isolated CODEX_HOME paths must be disjoint");
    }

    reject_reparse_path(daily_codex_home)?;
    let source_root = daily_codex_home.join("skills");
    reject_reparse_path(&source_root)?;
    if !source_root.is_dir() {
        return Ok(NativeSkillSyncReceipt::default());
    }
    reject_reparse_path(isolated_codex_home)?;
    let destination_root = isolated_codex_home.join("skills");
    fs::create_dir_all(&destination_root).with_context(|| {
        format!(
            "failed to create isolated Skills directory {}",
            destination_root.display()
        )
    })?;
    reject_reparse_path(&destination_root)?;

    let manifest_path = isolated_codex_home.join(MANIFEST_NAME);
    reject_reparse_path(&manifest_path)?;
    let mut manifest = load_manifest(&manifest_path)?;
    let mut receipt = NativeSkillSyncReceipt::default();
    let mut seen = BTreeSet::new();
    project_directory(
        &source_root,
        &source_root,
        &destination_root,
        &mut manifest,
        &mut seen,
        &mut receipt,
    )?;
    remove_deleted_projections(&destination_root, &mut manifest, &seen, &mut receipt)?;
    let rendered = serde_json::to_vec_pretty(&manifest)?;
    install_bootstrap_atomically(&manifest_path, &rendered)?;
    Ok(receipt)
}

fn project_directory(
    source_root: &Path,
    directory: &Path,
    destination_root: &Path,
    manifest: &mut SkillProjectionManifest,
    seen: &mut BTreeSet<String>,
    receipt: &mut NativeSkillSyncReceipt,
) -> Result<()> {
    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed to enumerate Skills path {}", directory.display()))?
    {
        let entry = entry?;
        let source = entry.path();
        let relative = source
            .strip_prefix(source_root)
            .context("Skill source escaped its root")?;
        if should_ignore_relative_path(relative) {
            receipt.skipped += 1;
            continue;
        }
        let key = relative_path_key(relative)?;
        let metadata = fs::symlink_metadata(&source)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            preserve_unsafe_source_projections(
                &key,
                "source_reparse_point",
                destination_root,
                manifest,
                seen,
                receipt,
            )?;
            receipt.skipped += 1;
            continue;
        }
        if metadata.is_dir() {
            project_directory(
                source_root,
                &source,
                destination_root,
                manifest,
                seen,
                receipt,
            )?;
            continue;
        }
        if !metadata.is_file() {
            preserve_unsafe_source_projections(
                &key,
                "source_not_regular",
                destination_root,
                manifest,
                seen,
                receipt,
            )?;
            receipt.skipped += 1;
            continue;
        }
        seen.insert(key.clone());
        let content = match read_stable_source_file(&source)? {
            SourceFileState::Content(content) => content,
            SourceFileState::HardLink => {
                preserve_unsafe_source_projections(
                    &key,
                    "source_hard_link",
                    destination_root,
                    manifest,
                    seen,
                    receipt,
                )?;
                receipt.skipped += 1;
                continue;
            }
            SourceFileState::NotRegular => {
                preserve_unsafe_source_projections(
                    &key,
                    "source_not_regular",
                    destination_root,
                    manifest,
                    seen,
                    receipt,
                )?;
                receipt.skipped += 1;
                continue;
            }
            SourceFileState::ReparsePoint => {
                preserve_unsafe_source_projections(
                    &key,
                    "source_reparse_point",
                    destination_root,
                    manifest,
                    seen,
                    receipt,
                )?;
                receipt.skipped += 1;
                continue;
            }
            SourceFileState::TooLarge => {
                preserve_unsafe_source_projections(
                    &key,
                    "source_too_large",
                    destination_root,
                    manifest,
                    seen,
                    receipt,
                )?;
                receipt.skipped += 1;
                continue;
            }
            SourceFileState::Unstable => {
                preserve_unsafe_source_projections(
                    &key,
                    "source_changed_during_projection",
                    destination_root,
                    manifest,
                    seen,
                    receipt,
                )?;
                receipt.skipped += 1;
                continue;
            }
        };
        let destination = destination_root.join(relative);
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Skill destination has no parent"))?;
        reject_reparse_path(parent)?;
        fs::create_dir_all(parent)?;
        reject_reparse_path(parent)?;
        let source_sha256 = sha256_bytes(&content);
        match manifest.files.get(&key) {
            Some(previous) => {
                let destination_state = inspect_destination(&destination)?;
                let destination_sha256 = match &destination_state {
                    DestinationState::HardLink(sha256) | DestinationState::Regular(sha256) => {
                        Some(sha256.clone())
                    }
                    _ => None,
                };
                if matches!(destination_state, DestinationState::HardLink(_))
                    || destination_sha256.as_deref() != Some(&previous.destination_sha256)
                {
                    manifest.conflicts.insert(
                        key,
                        SkillProjectionConflict {
                            reason: match destination_state {
                                DestinationState::HardLink(_) => "destination_hard_link",
                                DestinationState::Missing => "destination_missing",
                                DestinationState::NotRegular => "destination_not_regular",
                                DestinationState::ReparsePoint => "destination_reparse_point",
                                DestinationState::Regular(_) => "destination_modified",
                            }
                            .into(),
                            source_sha256: Some(source_sha256),
                            destination_sha256,
                        },
                    );
                    receipt.conflicts += 1;
                    continue;
                }
                if source_sha256 == previous.source_sha256 {
                    manifest.conflicts.remove(&key);
                    receipt.unchanged += 1;
                    continue;
                }
                install_bootstrap_atomically(&destination, &content)?;
                manifest.conflicts.remove(&key);
                manifest.files.insert(
                    key,
                    SkillProjectionRecord {
                        source_sha256: source_sha256.clone(),
                        destination_sha256: source_sha256,
                    },
                );
                receipt.updated += 1;
            }
            None => {
                match inspect_destination(&destination)? {
                    DestinationState::Missing => {}
                    DestinationState::HardLink(destination_sha256) => {
                        manifest.conflicts.insert(
                            key,
                            SkillProjectionConflict {
                                reason: "destination_hard_link".into(),
                                source_sha256: Some(source_sha256),
                                destination_sha256: Some(destination_sha256),
                            },
                        );
                        receipt.conflicts += 1;
                        continue;
                    }
                    DestinationState::ReparsePoint => {
                        manifest.conflicts.insert(
                            key,
                            SkillProjectionConflict {
                                reason: "destination_reparse_point".into(),
                                source_sha256: Some(source_sha256),
                                destination_sha256: None,
                            },
                        );
                        receipt.conflicts += 1;
                        continue;
                    }
                    DestinationState::NotRegular => {
                        manifest.conflicts.insert(
                            key,
                            SkillProjectionConflict {
                                reason: "unmanaged_destination".into(),
                                source_sha256: Some(source_sha256),
                                destination_sha256: None,
                            },
                        );
                        receipt.conflicts += 1;
                        continue;
                    }
                    DestinationState::Regular(destination_sha256) => {
                        manifest.conflicts.insert(
                            key,
                            SkillProjectionConflict {
                                reason: "unmanaged_destination".into(),
                                source_sha256: Some(source_sha256),
                                destination_sha256: Some(destination_sha256),
                            },
                        );
                        receipt.conflicts += 1;
                        continue;
                    }
                }
                install_bootstrap_atomically(&destination, &content)?;
                manifest.conflicts.remove(&key);
                manifest.files.insert(
                    key,
                    SkillProjectionRecord {
                        source_sha256: source_sha256.clone(),
                        destination_sha256: source_sha256,
                    },
                );
                receipt.projected += 1;
            }
        }
    }
    Ok(())
}

fn preserve_unsafe_source_projections(
    key: &str,
    reason: &str,
    destination_root: &Path,
    manifest: &mut SkillProjectionManifest,
    seen: &mut BTreeSet<String>,
    receipt: &mut NativeSkillSyncReceipt,
) -> Result<()> {
    let prefix = format!("{key}/");
    let managed = manifest
        .files
        .keys()
        .filter(|candidate| candidate.as_str() == key || candidate.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    for managed_key in managed {
        seen.insert(managed_key.clone());
        let destination_sha256 = match inspect_destination(&destination_root.join(&managed_key))? {
            DestinationState::HardLink(sha256) | DestinationState::Regular(sha256) => Some(sha256),
            DestinationState::Missing
            | DestinationState::NotRegular
            | DestinationState::ReparsePoint => None,
        };
        manifest.conflicts.insert(
            managed_key,
            SkillProjectionConflict {
                reason: reason.into(),
                source_sha256: None,
                destination_sha256,
            },
        );
        receipt.conflicts += 1;
    }
    Ok(())
}

fn remove_deleted_projections(
    destination_root: &Path,
    manifest: &mut SkillProjectionManifest,
    seen: &BTreeSet<String>,
    receipt: &mut NativeSkillSyncReceipt,
) -> Result<()> {
    let deleted = manifest
        .files
        .keys()
        .filter(|key| !seen.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    for key in deleted {
        let previous = manifest
            .files
            .get(&key)
            .expect("deleted projection key came from the manifest");
        let destination = destination_root.join(&key);
        let destination_sha256 = match inspect_destination(&destination)? {
            DestinationState::Missing => {
                manifest.files.remove(&key);
                manifest.conflicts.remove(&key);
                continue;
            }
            DestinationState::Regular(sha256) => sha256,
            DestinationState::HardLink(sha256) => {
                manifest.conflicts.insert(
                    key,
                    SkillProjectionConflict {
                        reason: "source_deleted_destination_hard_link".into(),
                        source_sha256: None,
                        destination_sha256: Some(sha256),
                    },
                );
                receipt.conflicts += 1;
                continue;
            }
            DestinationState::ReparsePoint => {
                manifest.conflicts.insert(
                    key,
                    SkillProjectionConflict {
                        reason: "source_deleted_destination_reparse_point".into(),
                        source_sha256: None,
                        destination_sha256: None,
                    },
                );
                receipt.conflicts += 1;
                continue;
            }
            DestinationState::NotRegular => {
                manifest.conflicts.insert(
                    key,
                    SkillProjectionConflict {
                        reason: "source_deleted_destination_not_regular".into(),
                        source_sha256: None,
                        destination_sha256: None,
                    },
                );
                receipt.conflicts += 1;
                continue;
            }
        };
        if destination_sha256 != previous.destination_sha256 {
            manifest.conflicts.insert(
                key,
                SkillProjectionConflict {
                    reason: "source_deleted_destination_modified".into(),
                    source_sha256: None,
                    destination_sha256: Some(destination_sha256),
                },
            );
            receipt.conflicts += 1;
            continue;
        }
        fs::remove_file(&destination)?;
        prune_empty_projection_directories(&destination, destination_root)?;
        manifest.files.remove(&key);
        manifest.conflicts.remove(&key);
        receipt.removed += 1;
    }
    Ok(())
}

fn prune_empty_projection_directories(path: &Path, destination_root: &Path) -> Result<()> {
    let mut current = path.parent();
    while let Some(directory) = current {
        if directory == destination_root || !directory.starts_with(destination_root) {
            break;
        }
        let metadata = match fs::symlink_metadata(directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                current = directory.parent();
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            break;
        }
        match fs::remove_dir(directory) {
            Ok(()) => current = directory.parent(),
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn load_manifest(path: &Path) -> Result<SkillProjectionManifest> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SkillProjectionManifest {
                version: MANIFEST_VERSION,
                files: BTreeMap::new(),
                conflicts: BTreeMap::new(),
            });
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > MAX_MANIFEST_BYTES
    {
        bail!("isolated Skill projection manifest has an invalid file shape or size");
    }
    let mut file = open_shared_no_follow(path)?;
    let initial = stable_file_snapshot(&file)?;
    if initial.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || initial.attributes & FILE_ATTRIBUTE_DIRECTORY != 0
        || initial.link_count != 1
        || initial.len > MAX_MANIFEST_BYTES
    {
        bail!("isolated Skill projection manifest has an invalid file shape or size");
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != initial.len
        || bytes.len() as u64 > MAX_MANIFEST_BYTES
        || stable_file_snapshot(&file)? != initial
    {
        bail!("isolated Skill projection manifest changed during read");
    }
    let manifest: SkillProjectionManifest =
        serde_json::from_slice(&bytes).context("isolated Skill projection manifest is invalid")?;
    if manifest.version != MANIFEST_VERSION {
        bail!("isolated Skill projection manifest version is unsupported");
    }
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &SkillProjectionManifest) -> Result<()> {
    let mut case_folded_files = BTreeSet::new();
    for (key, record) in &manifest.files {
        validate_manifest_key(key)?;
        if !case_folded_files.insert(key.to_ascii_lowercase()) {
            bail!("isolated Skill projection manifest contains a duplicate relative path");
        }
        validate_sha256(&record.source_sha256)?;
        validate_sha256(&record.destination_sha256)?;
    }
    for (key, conflict) in &manifest.conflicts {
        validate_manifest_key(key)?;
        if conflict.reason.is_empty()
            || conflict.reason.len() > 128
            || !conflict
                .reason
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        {
            bail!("isolated Skill projection manifest contains an invalid conflict reason");
        }
        if let Some(sha256) = &conflict.source_sha256 {
            validate_sha256(sha256)?;
        }
        if let Some(sha256) = &conflict.destination_sha256 {
            validate_sha256(sha256)?;
        }
    }
    Ok(())
}

fn validate_manifest_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 4096 || key.contains(['\\', '\0']) {
        bail!("isolated Skill projection manifest contains an invalid relative path");
    }
    let path = Path::new(key);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || should_ignore_relative_path(path)
    {
        bail!("isolated Skill projection manifest contains an invalid relative path");
    }
    Ok(())
}

fn validate_sha256(sha256: &str) -> Result<()> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        bail!("isolated Skill projection manifest contains an invalid SHA-256 value");
    }
    Ok(())
}

fn relative_path_key(path: &Path) -> Result<String> {
    let parts = path
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow::anyhow!("Skill relative path is not valid Unicode"))?;
    Ok(parts.join("/"))
}

fn should_ignore_relative_path(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if components.first().is_some_and(|name| name == ".system")
        || components.iter().any(|name| {
            matches!(
                name.as_str(),
                ".git"
                    | ".hg"
                    | ".svn"
                    | ".cache"
                    | ".mypy_cache"
                    | ".pytest_cache"
                    | ".ruff_cache"
                    | "__pycache__"
                    | "cache"
                    | "caches"
                    | "node_modules"
                    | "target"
                    | "temp"
                    | "tmp"
            )
        })
    {
        return true;
    }
    let Some(name) = components.last() else {
        return true;
    };
    if matches!(name.as_str(), ".ds_store" | "thumbs.db") || name.ends_with('~') {
        return true;
    }
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension,
                "bak" | "old" | "pyc" | "pyo" | "swp" | "swo" | "temp" | "tmp"
            )
        })
}

fn reject_reparse_path(path: &Path) -> Result<()> {
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                bail!("native Skill sync path contains a reparse point");
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn inspect_destination(path: &Path) -> Result<DestinationState> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DestinationState::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Ok(DestinationState::ReparsePoint);
    }
    if !metadata.is_file() {
        return Ok(DestinationState::NotRegular);
    }
    let mut file = match open_shared_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DestinationState::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    let initial = stable_file_snapshot(&file)?;
    if initial.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Ok(DestinationState::ReparsePoint);
    }
    if initial.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 || initial.len > MAX_SKILL_FILE_BYTES {
        return Ok(DestinationState::NotRegular);
    }
    let hard_linked = initial.link_count != 1;
    let mut content = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_SKILL_FILE_BYTES + 1)
        .read_to_end(&mut content)?;
    if content.len() as u64 != initial.len
        || content.len() as u64 > MAX_SKILL_FILE_BYTES
        || stable_file_snapshot(&file)? != initial
    {
        return Ok(DestinationState::NotRegular);
    }
    let sha256 = sha256_bytes(&content);
    if hard_linked {
        Ok(DestinationState::HardLink(sha256))
    } else {
        Ok(DestinationState::Regular(sha256))
    }
}

fn sha256_bytes(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn read_stable_source_file(path: &Path) -> Result<SourceFileState> {
    let mut file = match open_shared_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SourceFileState::Unstable);
        }
        Err(error) => return Err(error.into()),
    };
    let initial = stable_file_snapshot(&file)?;
    if initial.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Ok(SourceFileState::ReparsePoint);
    }
    if initial.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Ok(SourceFileState::NotRegular);
    }
    if initial.link_count != 1 {
        return Ok(SourceFileState::HardLink);
    }
    if initial.len > MAX_SKILL_FILE_BYTES {
        return Ok(SourceFileState::TooLarge);
    }
    let mut content = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_SKILL_FILE_BYTES + 1)
        .read_to_end(&mut content)?;
    if content.len() as u64 != initial.len
        || content.len() as u64 > MAX_SKILL_FILE_BYTES
        || stable_file_snapshot(&file)? != initial
    {
        return Ok(SourceFileState::Unstable);
    }
    Ok(SourceFileState::Content(content))
}

fn open_shared_no_follow(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

fn stable_file_snapshot(file: &File) -> Result<StableFileSnapshot> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    let result = unsafe {
        GetFileInformationByHandle(
            file.as_raw_handle() as HANDLE,
            &mut information as *mut BY_HANDLE_FILE_INFORMATION,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(StableFileSnapshot {
        attributes: information.dwFileAttributes,
        file_index: ((information.nFileIndexHigh as u64) << 32) | information.nFileIndexLow as u64,
        last_write_high: information.ftLastWriteTime.dwHighDateTime,
        last_write_low: information.ftLastWriteTime.dwLowDateTime,
        len: ((information.nFileSizeHigh as u64) << 32) | information.nFileSizeLow as u64,
        link_count: information.nNumberOfLinks,
        volume_serial: information.dwVolumeSerialNumber,
    })
}
