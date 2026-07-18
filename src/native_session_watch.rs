use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    time::{Duration, Instant, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_NOTIFY_CHANGE_SIZE, FindCloseChangeNotification, FindFirstChangeNotificationW,
        FindNextChangeNotification,
    },
    System::Threading::WaitForSingleObject,
};

use crate::NativeSharedSessionRollout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RolloutSignature {
    len: u64,
    modified_nanos: u128,
}

#[derive(Debug)]
struct TrackedRollout {
    thread_id: String,
    path: PathBuf,
    signature: Option<RolloutSignature>,
}

struct ChangeNotification(HANDLE);

impl ChangeNotification {
    fn new(path: &Path) -> Result<Self> {
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            FindFirstChangeNotificationW(
                wide.as_ptr(),
                1,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE
                    | FILE_NOTIFY_CHANGE_SIZE,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("failed to watch session directory {}", path.display()));
        }
        Ok(Self(handle))
    }

    fn poll(&self) -> Result<bool> {
        match unsafe { WaitForSingleObject(self.0, 0) } {
            WAIT_TIMEOUT => Ok(false),
            WAIT_OBJECT_0 => {
                if unsafe { FindNextChangeNotification(self.0) } == 0 {
                    return Err(std::io::Error::last_os_error())
                        .context("failed to continue session directory notifications");
                }
                Ok(true)
            }
            status => bail!("failed to poll session directory notification (wait={status})"),
        }
    }
}

impl Drop for ChangeNotification {
    fn drop(&mut self) {
        unsafe {
            FindCloseChangeNotification(self.0);
        }
    }
}

pub struct NativeSessionChangeMonitor {
    watches: Vec<ChangeNotification>,
    tracked: Vec<TrackedRollout>,
    pending: BTreeSet<String>,
    last_change: Option<Instant>,
    debounce: Duration,
}

impl NativeSessionChangeMonitor {
    pub fn new<T>(
        daily_codex_home: &Path,
        isolated_codex_home: &Path,
        rollouts: T,
        debounce: Duration,
    ) -> Result<Self>
    where
        T: IntoIterator<Item = NativeSharedSessionRollout>,
    {
        if debounce.is_zero() {
            bail!("session change debounce must be positive");
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
            bail!("session change roots must be disjoint");
        }

        let mut watches = Vec::new();
        for home in [&daily, &isolated] {
            for directory in ["sessions", "archived_sessions"] {
                let path = home.join(directory);
                if safe_directory(&path)? {
                    watches.push(ChangeNotification::new(&path)?);
                }
            }
        }
        if watches.is_empty() {
            bail!("session change monitor found no session directories");
        }

        let mut unique_paths = BTreeMap::<PathBuf, String>::new();
        for rollout in rollouts {
            if rollout.thread_id.trim().is_empty() {
                bail!("tracked session thread id must not be empty");
            }
            for path in [rollout.daily_path, rollout.isolated_path] {
                if !path.starts_with(&daily) && !path.starts_with(&isolated) {
                    bail!("tracked rollout is outside the two CODEX_HOME roots");
                }
                if unique_paths
                    .insert(path, rollout.thread_id.clone())
                    .is_some()
                {
                    bail!("tracked session rollout path is duplicated");
                }
            }
        }
        let tracked = unique_paths
            .into_iter()
            .map(|(path, thread_id)| {
                Ok(TrackedRollout {
                    thread_id,
                    signature: rollout_signature(&path)?,
                    path,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            watches,
            tracked,
            pending: BTreeSet::new(),
            last_change: None,
            debounce,
        })
    }

    pub fn poll_changed(&mut self) -> Result<Vec<String>> {
        let signaled = self
            .watches
            .iter()
            .map(ChangeNotification::poll)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .any(|changed| changed);
        let now = Instant::now();
        if signaled {
            let mut changed = false;
            for rollout in &mut self.tracked {
                let current = rollout_signature(&rollout.path)?;
                if current != rollout.signature {
                    rollout.signature = current;
                    self.pending.insert(rollout.thread_id.clone());
                    changed = true;
                }
            }
            if changed {
                self.last_change = Some(now);
            }
        }
        if !self.pending.is_empty()
            && self
                .last_change
                .is_some_and(|last_change| now.duration_since(last_change) >= self.debounce)
        {
            return Ok(std::mem::take(&mut self.pending).into_iter().collect());
        }
        Ok(Vec::new())
    }
}

fn safe_directory(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                bail!("session change directory is reparse-backed");
            }
            Ok(metadata.is_dir())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn rollout_signature(path: &Path) -> Result<Option<RolloutSignature>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        bail!("tracked session rollout became reparse-backed");
    }
    if !metadata.is_file() {
        return Ok(None);
    }
    let modified_nanos = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .context("tracked session rollout modification time predates the Unix epoch")?
        .as_nanos();
    Ok(Some(RolloutSignature {
        len: metadata.len(),
        modified_nanos,
    }))
}
