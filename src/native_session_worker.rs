use std::{
    collections::BTreeSet,
    ffi::c_void,
    fs,
    io::{Read, Write},
    mem::size_of,
    os::windows::{io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    ptr::null,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    },
};

use crate::{
    NativeSessionChangeMonitor, NativeSessionContinuityReceipt, environment_variable_is_sensitive,
    install_bootstrap_atomically, observe_native_session_continuity_via_official_app_server,
};

const WORKER_REQUEST_VERSION: u8 = 1;
const MAX_WORKER_REQUEST_BYTES: u64 = 512 * 1024;
const MAX_WORKER_THREADS: usize = 4096;
const MAX_WORKER_DIAGNOSTIC_BYTES: usize = 4096;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const WORKER_SUBCOMMAND: &str = "session-continuity-worker";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSessionContinuityWorkerOutcome {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub diagnostic: String,
}

pub trait NativeSessionContinuityWorker {
    fn try_finish(&mut self) -> Result<Option<NativeSessionContinuityWorkerOutcome>>;

    fn terminate(&mut self);
}

pub trait NativeSessionContinuityWorkerBackend {
    fn start(&mut self, thread_ids: &[String]) -> Result<Box<dyn NativeSessionContinuityWorker>>;
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeSessionContinuityWorkerRequest {
    version: u8,
    thread_ids: Vec<String>,
}

pub struct NativeSessionContinuityProcessBackend {
    executable: PathBuf,
    daily_codex_home: PathBuf,
    isolated_codex_home: PathBuf,
    request_path: PathBuf,
    manifest_path: PathBuf,
    app_server_program: Option<PathBuf>,
    job: KillOnCloseJob,
}

impl NativeSessionContinuityProcessBackend {
    pub fn new(
        executable: PathBuf,
        daily_codex_home: PathBuf,
        isolated_codex_home: PathBuf,
        request_path: PathBuf,
        manifest_path: PathBuf,
    ) -> Result<Self> {
        let executable = canonical_regular_file(&executable, "continuity worker executable")?;
        let daily_codex_home = fs::canonicalize(&daily_codex_home).with_context(|| {
            format!(
                "failed to resolve daily CODEX_HOME {}",
                daily_codex_home.display()
            )
        })?;
        let isolated_codex_home = fs::canonicalize(&isolated_codex_home).with_context(|| {
            format!(
                "failed to resolve isolated CODEX_HOME {}",
                isolated_codex_home.display()
            )
        })?;
        if daily_codex_home == isolated_codex_home {
            bail!("continuity worker CODEX_HOME paths must be disjoint");
        }
        validate_worker_owned_path(&request_path, &isolated_codex_home, "request")?;
        validate_worker_owned_path(&manifest_path, &isolated_codex_home, "manifest")?;
        if request_path == manifest_path {
            bail!("continuity worker request and manifest paths must differ");
        }
        Ok(Self {
            executable,
            daily_codex_home,
            isolated_codex_home,
            request_path,
            manifest_path,
            app_server_program: None,
            job: KillOnCloseJob::create()?,
        })
    }

    pub fn with_app_server_program(mut self, program: PathBuf) -> Result<Self> {
        self.app_server_program = Some(canonical_regular_file(
            &program,
            "official app-server program",
        )?);
        Ok(self)
    }
}

impl NativeSessionContinuityWorkerBackend for NativeSessionContinuityProcessBackend {
    fn start(&mut self, thread_ids: &[String]) -> Result<Box<dyn NativeSessionContinuityWorker>> {
        write_worker_request(&self.request_path, thread_ids)?;
        let mut command = Command::new(&self.executable);
        command
            .arg(WORKER_SUBCOMMAND)
            .arg("--daily-codex-home")
            .arg(&self.daily_codex_home)
            .arg("--isolated-codex-home")
            .arg(&self.isolated_codex_home)
            .arg("--request")
            .arg(&self.request_path)
            .arg("--manifest")
            .arg(&self.manifest_path)
            .arg("--parent-gated")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        for (key, _) in std::env::vars_os() {
            if environment_variable_is_sensitive(&key) {
                command.env_remove(key);
            }
        }
        if let Some(program) = &self.app_server_program {
            command.env("CODEX_ADMINISTRATOR_CODEX_APP_SERVER", program);
        }
        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to start continuity worker {}",
                self.executable.display()
            )
        })?;
        if let Err(error) = self.job.assign(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let mut gate = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("continuity worker gate was unavailable"))?;
        if let Err(error) = gate.write_all(b"1") {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).context("failed to release the continuity worker gate");
        }
        drop(gate);
        Ok(Box::new(ProcessContinuityWorker { child: Some(child) }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSessionContinuityWorkerFinished {
    pub thread_ids: Vec<String>,
    pub outcome: NativeSessionContinuityWorkerOutcome,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeSessionContinuityMaintenance {
    pub changed_threads: Vec<String>,
    pub started_threads: Vec<String>,
    pub finished: Option<NativeSessionContinuityWorkerFinished>,
    pub running: bool,
}

struct ActiveWorker {
    thread_ids: Vec<String>,
    worker: Box<dyn NativeSessionContinuityWorker>,
}

pub struct NativeSessionContinuityCoordinator {
    monitor: NativeSessionChangeMonitor,
    backend: Box<dyn NativeSessionContinuityWorkerBackend>,
    pending: BTreeSet<String>,
    active: Option<ActiveWorker>,
}

impl NativeSessionContinuityCoordinator {
    pub fn new(
        monitor: NativeSessionChangeMonitor,
        backend: Box<dyn NativeSessionContinuityWorkerBackend>,
    ) -> Self {
        Self {
            monitor,
            backend,
            pending: BTreeSet::new(),
            active: None,
        }
    }

    pub fn enqueue_threads<T>(&mut self, thread_ids: T) -> Result<()>
    where
        T: IntoIterator,
        T::Item: AsRef<str>,
    {
        for thread_id in thread_ids {
            let thread_id = thread_id.as_ref();
            validate_thread_id(thread_id)?;
            self.pending.insert(thread_id.to_owned());
            if self.pending.len() > MAX_WORKER_THREADS {
                bail!("continuity worker queue exceeds its thread limit");
            }
        }
        Ok(())
    }

    pub fn maintain_once(&mut self) -> Result<NativeSessionContinuityMaintenance> {
        let changed_threads = self.monitor.poll_changed()?;
        self.enqueue_threads(changed_threads.iter())?;

        let finished =
            match self.active.as_mut() {
                Some(active) => active.worker.try_finish()?.map(|outcome| {
                    NativeSessionContinuityWorkerFinished {
                        thread_ids: active.thread_ids.clone(),
                        outcome,
                    }
                }),
                None => None,
            };
        if finished.is_some() {
            self.active = None;
        }

        let started_threads = if self.active.is_none() && !self.pending.is_empty() {
            let batch = std::mem::take(&mut self.pending)
                .into_iter()
                .collect::<Vec<_>>();
            match self.backend.start(&batch) {
                Ok(worker) => {
                    self.active = Some(ActiveWorker {
                        thread_ids: batch.clone(),
                        worker,
                    });
                    batch
                }
                Err(error) => {
                    self.pending.extend(batch);
                    return Err(error);
                }
            }
        } else {
            Vec::new()
        };

        Ok(NativeSessionContinuityMaintenance {
            changed_threads,
            started_threads,
            finished,
            running: self.active.is_some(),
        })
    }
}

impl Drop for NativeSessionContinuityCoordinator {
    fn drop(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.worker.terminate();
        }
    }
}

pub fn run_native_session_continuity_worker(
    daily_codex_home: &Path,
    isolated_codex_home: &Path,
    request_path: &Path,
    manifest_path: &Path,
) -> Result<NativeSessionContinuityReceipt> {
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
        bail!("continuity worker CODEX_HOME paths must be disjoint");
    }
    validate_worker_owned_path(request_path, &isolated, "request")?;
    validate_worker_owned_path(manifest_path, &isolated, "manifest")?;
    let request = read_worker_request(request_path)?;
    observe_native_session_continuity_via_official_app_server(
        &daily,
        &isolated,
        request.thread_ids.iter().map(String::as_str),
        manifest_path,
    )?
    .ok_or_else(|| anyhow::anyhow!("continuity worker could not locate an official app-server"))
}

struct ProcessContinuityWorker {
    child: Option<Child>,
}

impl NativeSessionContinuityWorker for ProcessContinuityWorker {
    fn try_finish(&mut self) -> Result<Option<NativeSessionContinuityWorkerOutcome>> {
        let Some(child) = self.child.as_mut() else {
            return Ok(None);
        };
        let Some(status) = child
            .try_wait()
            .context("failed to query continuity worker status")?
        else {
            return Ok(None);
        };
        let mut child = self
            .child
            .take()
            .expect("continuity worker child disappeared");
        child
            .wait()
            .context("failed to reap completed continuity worker")?;
        let stdout = read_bounded_stream(child.stdout.take())?;
        let stderr = read_bounded_stream(child.stderr.take())?;
        let diagnostic = if status.success() {
            String::new()
        } else {
            bounded_diagnostic(&stderr, &stdout)
        };
        Ok(Some(NativeSessionContinuityWorkerOutcome {
            success: status.success(),
            exit_code: status.code(),
            diagnostic,
        }))
    }

    fn terminate(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ProcessContinuityWorker {
    fn drop(&mut self) {
        self.terminate();
    }
}

struct KillOnCloseJob(HANDLE);

impl KillOnCloseJob {
    fn create() -> Result<Self> {
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error())
                .context("failed to create continuity worker job");
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            let error = std::io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(error).context("failed to configure continuity worker cleanup");
        }
        Ok(Self(handle))
    }

    fn assign(&self, child: &Child) -> Result<()> {
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) } == 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to assign continuity worker to its owned job");
        }
        Ok(())
    }
}

impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn write_worker_request(path: &Path, thread_ids: &[String]) -> Result<()> {
    let mut unique = BTreeSet::new();
    for thread_id in thread_ids {
        validate_thread_id(thread_id)?;
        if !unique.insert(thread_id.clone()) {
            bail!("continuity worker thread ids must be unique and non-empty");
        }
        if unique.len() > MAX_WORKER_THREADS {
            bail!("continuity worker request exceeds its thread limit");
        }
    }
    if unique.is_empty() {
        bail!("continuity worker request must contain at least one thread");
    }
    let request = NativeSessionContinuityWorkerRequest {
        version: WORKER_REQUEST_VERSION,
        thread_ids: unique.into_iter().collect(),
    };
    let content = serde_json::to_vec_pretty(&request)?;
    if content.len() as u64 > MAX_WORKER_REQUEST_BYTES {
        bail!("continuity worker request exceeds its size limit");
    }
    install_bootstrap_atomically(path, &content).map(|_| ())
}

fn read_worker_request(path: &Path) -> Result<NativeSessionContinuityWorkerRequest> {
    let metadata = fs::metadata(path).with_context(|| {
        format!(
            "failed to inspect continuity worker request {}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.len() > MAX_WORKER_REQUEST_BYTES {
        bail!("continuity worker request is not a bounded regular file");
    }
    let request: NativeSessionContinuityWorkerRequest =
        serde_json::from_slice(&fs::read(path).with_context(|| {
            format!(
                "failed to read continuity worker request {}",
                path.display()
            )
        })?)
        .context("continuity worker request is invalid")?;
    if request.version != WORKER_REQUEST_VERSION {
        bail!("continuity worker request version is unsupported");
    }
    let mut unique = BTreeSet::new();
    for thread_id in &request.thread_ids {
        if validate_thread_id(thread_id).is_err() || !unique.insert(thread_id.as_str()) {
            bail!("continuity worker request contains invalid thread ids");
        }
    }
    if unique.is_empty() || unique.len() > MAX_WORKER_THREADS {
        bail!("continuity worker request has an invalid thread count");
    }
    Ok(request)
}

fn validate_worker_owned_path(path: &Path, isolated_home: &Path, label: &str) -> Result<()> {
    if !path.is_absolute() {
        bail!("continuity worker {label} path must be absolute");
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("continuity worker {label} path has no parent"))?;
    let parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve continuity worker {label} parent {}",
            parent.display()
        )
    })?;
    if parent != isolated_home {
        bail!("continuity worker {label} must be stored directly in isolated CODEX_HOME");
    }
    Ok(())
}

fn canonical_regular_file(path: &Path, label: &str) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("{label} must be absolute");
    }
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {label} {}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("{label} must be a regular non-link file");
    }
    fs::canonicalize(path).with_context(|| format!("failed to resolve {label} {}", path.display()))
}

fn read_bounded_stream<T: Read>(stream: Option<T>) -> Result<Vec<u8>> {
    let Some(mut stream) = stream else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    stream
        .by_ref()
        .take((MAX_WORKER_DIAGNOSTIC_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_WORKER_DIAGNOSTIC_BYTES {
        bytes.drain(..bytes.len() - MAX_WORKER_DIAGNOSTIC_BYTES);
    }
    Ok(bytes)
}

fn bounded_diagnostic(stderr: &[u8], stdout: &[u8]) -> String {
    let bytes = if stderr.is_empty() { stdout } else { stderr };
    String::from_utf8_lossy(bytes)
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or("continuity worker failed without a diagnostic")
        .chars()
        .take(MAX_WORKER_DIAGNOSTIC_BYTES)
        .collect()
}

fn validate_thread_id(thread_id: &str) -> Result<()> {
    let bytes = thread_id.as_bytes();
    if bytes.len() != 36
        || ![8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| ![8, 13, 18, 23].contains(&index) && !byte.is_ascii_hexdigit())
    {
        bail!("continuity worker thread id is invalid");
    }
    Ok(())
}
