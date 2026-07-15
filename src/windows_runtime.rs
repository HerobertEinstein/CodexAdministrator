use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString, c_void},
    fs,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
        FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP_STATE_LISTEN, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    },
    Networking::WinSock::AF_INET,
    Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
    Storage::Packaging::Appx::GetPackageFamilyName,
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Threading::{
            CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
            GetProcessTimes, OpenProcess, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_TERMINATE, QueryFullProcessImageNameW, ResumeThread, STARTUPINFOW,
            TerminateProcess, WaitForSingleObject,
        },
    },
};

const OFFICIAL_PACKAGE_FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";
const PROCESS_SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
const DESCENDANT_QUIESCENCE_PASSES: usize = 40;

use crate::{
    DirectCdpTarget, DirectIsolationContract, DirectRuntimeBackend, GrokNativeProviderConfig,
    install_grok_native_provider,
};

pub struct WindowsDirectRuntime {
    root: PathBuf,
    provider: Option<GrokNativeProviderConfig>,
    job: Option<OwnedJob>,
    owns_root: bool,
    cdp: crate::LoopbackCdpClient,
}

pub fn validate_official_chatgpt_executable(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!(
            "official ChatGPT/Codex executable does not exist: {}",
            path.display()
        );
    }
    if !path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("ChatGPT.exe"))
        || !path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("app"))
    {
        bail!("official host must be the packaged ChatGPT.exe app executable");
    }
    let package = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    let windows_apps = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    if !package.is_some_and(|value| {
        value.starts_with("OpenAI.Codex_") && value.ends_with("__2p2nqsd0c76g0")
    }) || !windows_apps.is_some_and(|value| value.eq_ignore_ascii_case("WindowsApps"))
    {
        bail!("official host is not inside an OpenAI.Codex Windows package");
    }
    Ok(())
}

pub fn validate_launchable_official_chatgpt_executable(path: &Path) -> Result<()> {
    validate_official_chatgpt_executable(path)?;
    let windows_apps = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("official package path has no WindowsApps root"))?;
    let container = windows_apps
        .parent()
        .ok_or_else(|| anyhow::anyhow!("WindowsApps path has no volume root"))?;
    let directly_on_volume = container.parent().is_none();
    let under_program_files = container
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("Program Files"))
        && container
            .parent()
            .is_some_and(|parent| parent.parent().is_none());
    if !directly_on_volume && !under_program_files {
        bail!("direct launch is restricted to the system WindowsApps package root");
    }
    Ok(())
}

pub fn find_official_chatgpt_executable() -> Result<PathBuf> {
    let mut paths = BTreeMap::<String, PathBuf>::new();
    let mut rejected = Vec::new();
    for pid in snapshot_processes_named("ChatGPT.exe")? {
        let Some(path) = process_image_path(pid) else {
            rejected.push(format!("pid {pid}: image path unavailable"));
            continue;
        };
        match validate_launchable_official_chatgpt_executable(&path) {
            Ok(()) => {
                paths.insert(path.to_string_lossy().to_lowercase(), path);
            }
            Err(error) => rejected.push(format!("pid {pid} ({}): {error}", path.display())),
        }
    }
    match paths.len() {
        1 => Ok(paths.into_values().next().unwrap()),
        0 => bail!(
            "unable to locate a running official ChatGPT/Codex package executable; {}",
            rejected.join(" | ")
        ),
        _ => bail!("multiple official ChatGPT/Codex package versions are running"),
    }
}

impl WindowsDirectRuntime {
    pub fn new(root: PathBuf, provider: Option<GrokNativeProviderConfig>) -> Result<Self> {
        if !root.is_absolute() {
            bail!("isolated Windows runtime root must be absolute");
        }
        if let Some(provider) = &provider {
            provider.validate()?;
        }
        Ok(Self {
            root,
            provider,
            job: None,
            owns_root: false,
            cdp: crate::LoopbackCdpClient::default(),
        })
    }

    fn remove_owned_root(&mut self) -> Result<()> {
        if !self.owns_root {
            return Ok(());
        }
        reject_reparse_ancestors(&self.root)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match fs::remove_dir_all(&self.root) {
                Ok(()) => {
                    self.owns_root = false;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.owns_root = false;
                    return Ok(());
                }
                Err(error) => {
                    if Instant::now() >= deadline {
                        return Err(error).with_context(|| {
                            format!(
                                "failed to remove isolated instance root {}",
                                self.root.display()
                            )
                        });
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

impl DirectRuntimeBackend for WindowsDirectRuntime {
    fn snapshot_chatgpt_pids(&mut self) -> Result<BTreeSet<u32>> {
        snapshot_processes_named("ChatGPT.exe")
    }

    fn prepare_owned_paths(&mut self, contract: &DirectIsolationContract) -> Result<()> {
        if self.job.is_some() || self.owns_root {
            bail!("isolated Windows runtime is already prepared");
        }
        contract.verify_owned_root(&self.root)?;
        let parent = self
            .root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("isolated runtime root has no parent"))?;
        reject_reparse_ancestors(parent)?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create isolated instance parent {}",
                parent.display()
            )
        })?;
        reject_reparse_ancestors(parent)?;
        fs::create_dir(&self.root).with_context(|| {
            format!(
                "isolated instance root already exists or cannot be owned: {}",
                self.root.display()
            )
        })?;
        self.owns_root = true;
        reject_reparse_ancestors(&self.root)?;

        let prepared = (|| -> Result<OwnedJob> {
            fs::create_dir(contract.isolated_profile()).with_context(|| {
                format!(
                    "failed to create isolated profile {}",
                    contract.isolated_profile().display()
                )
            })?;
            fs::create_dir(contract.isolated_codex_home()).with_context(|| {
                format!(
                    "failed to create isolated CODEX_HOME {}",
                    contract.isolated_codex_home().display()
                )
            })?;
            if let Some(provider) = &self.provider {
                install_grok_native_provider(
                    &contract.isolated_codex_home().join("config.toml"),
                    provider,
                )?;
            }
            OwnedJob::create()
        })();

        match prepared {
            Ok(job) => {
                self.job = Some(job);
                Ok(())
            }
            Err(error) => {
                let cleanup = self.remove_owned_root();
                if let Err(cleanup) = cleanup {
                    bail!("{error}; isolated root cleanup also failed: {cleanup}");
                }
                Err(error)
            }
        }
    }

    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
    ) -> Result<()> {
        self.job
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("isolated Windows runtime is not prepared"))?
            .launch(executable, arguments, environment, true)
    }

    fn wait_for_cdp_endpoint(&mut self, port: u16, timeout: Duration) -> Result<()> {
        self.cdp.wait_for_endpoint(port, timeout)
    }

    fn wait_for_app_target(&mut self, port: u16, timeout: Duration) -> Result<DirectCdpTarget> {
        self.cdp.wait_for_app_target(port, timeout)
    }

    fn owned_pids(&mut self) -> Result<BTreeSet<u32>> {
        self.job
            .as_mut()
            .map(OwnedJob::process_ids)
            .transpose()
            .map(Option::unwrap_or_default)
    }

    fn cdp_listener_pid(&mut self, port: u16) -> Result<u32> {
        loopback_listener_pid(port)
    }

    fn install_bootstrap(
        &mut self,
        target: &DirectCdpTarget,
        script: &str,
        timeout: Duration,
    ) -> Result<()> {
        self.cdp.install_bootstrap(target, script, timeout)
    }

    fn wait_for_ui_ready(&mut self, target: &DirectCdpTarget, timeout: Duration) -> Result<()> {
        self.cdp.wait_for_ui_ready(target, timeout)
    }

    fn wait_for_provider_ready(
        &mut self,
        target: &DirectCdpTarget,
        timeout: Duration,
    ) -> Result<()> {
        self.cdp.wait_for_provider_ready(target, timeout)
    }

    fn injection_healthy(&mut self, target: &DirectCdpTarget) -> Result<bool> {
        self.cdp.injection_healthy(target)
    }

    fn shutdown(&mut self) -> Result<()> {
        let job_result = self.job.take().map(OwnedJob::terminate).unwrap_or(Ok(()));
        let root_result = self.remove_owned_root();
        match (job_result, root_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(job), Err(root)) => {
                bail!("job cleanup failed: {job}; root cleanup failed: {root}")
            }
        }
    }
}

impl Drop for WindowsDirectRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProcessGeneration {
    created_at: u64,
    exited_at: Option<u64>,
}

impl ProcessGeneration {
    fn can_parent(self, child_created_at: u64, snapshot_at: u64) -> bool {
        child_created_at >= self.created_at
            && child_created_at <= snapshot_at
            && self
                .exited_at
                .is_none_or(|exited_at| child_created_at <= exited_at)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ProcessIdentity {
    pid: u32,
    created_at: u64,
}

struct TrackedProcess {
    handle: HANDLE,
    identity: ProcessIdentity,
}

impl TrackedProcess {
    fn from_handle(pid: u32, handle: HANDLE) -> Result<Self> {
        match process_times(handle) {
            Ok((created_at, _)) => Ok(Self {
                handle,
                identity: ProcessIdentity { pid, created_at },
            }),
            Err(error) => {
                unsafe {
                    CloseHandle(handle);
                }
                Err(error)
            }
        }
    }

    fn generation(&self) -> Result<ProcessGeneration> {
        let running = process_handle_is_running(self.handle)?;
        let (created_at, exited_at) = process_times(self.handle)?;
        if created_at != self.identity.created_at {
            bail!("tracked process handle changed identity");
        }
        if !running && exited_at == 0 {
            bail!("completed tracked process has no exit timestamp");
        }
        Ok(ProcessGeneration {
            created_at,
            exited_at: (!running).then_some(exited_at),
        })
    }
}

impl Drop for TrackedProcess {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = null_mut();
        }
    }
}

struct OwnedJob {
    handle: HANDLE,
    descendants: BTreeMap<ProcessIdentity, TrackedProcess>,
    uncertain_pids: BTreeSet<u32>,
}

impl OwnedJob {
    fn create() -> Result<Self> {
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error()).context("failed to create Windows job");
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
            return Err(error).context("failed to configure Windows job cleanup");
        }
        Ok(Self {
            handle,
            descendants: BTreeMap::new(),
            uncertain_pids: BTreeSet::new(),
        })
    }

    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
        verify_official: bool,
    ) -> Result<()> {
        if verify_official {
            validate_launchable_official_chatgpt_executable(executable)?;
        }
        let mut application = wide_null(executable.as_os_str());
        let mut command_line = windows_command_line(executable.as_os_str(), arguments);
        let environment = windows_environment_block(environment)?;
        let current_directory = executable
            .parent()
            .map(|path| wide_null(path.as_os_str()))
            .unwrap_or_else(|| vec![0]);
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut process = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                application.as_mut_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NEW_PROCESS_GROUP,
                environment.as_ptr() as *const c_void,
                current_directory.as_ptr(),
                &startup,
                &mut process,
            )
        };
        if created == 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "failed to create suspended process {}",
                    executable.display()
                )
            });
        }

        if verify_official
            && let Err(error) = validate_created_official_process(executable, process.hProcess)
        {
            return Err(cleanup_failed_process(error, process));
        }

        let assigned = unsafe { AssignProcessToJobObject(self.handle, process.hProcess) };
        if assigned == 0 {
            let error = anyhow::Error::new(std::io::Error::last_os_error())
                .context("failed to assign suspended process to the owned job");
            return Err(cleanup_failed_process(error, process));
        }
        let resumed = unsafe { ResumeThread(process.hThread) };
        if resumed == u32::MAX {
            let error = anyhow::Error::new(std::io::Error::last_os_error())
                .context("failed to resume owned process");
            return Err(cleanup_failed_process(error, process));
        }
        let created_at = match process_times(process.hProcess) {
            Ok((created_at, _)) => created_at,
            Err(error) => return Err(cleanup_failed_process(error, process)),
        };
        let identity = ProcessIdentity {
            pid: process.dwProcessId,
            created_at,
        };
        for existing in self
            .descendants
            .values()
            .filter(|existing| existing.identity.pid == process.dwProcessId)
        {
            match process_handle_is_running(existing.handle) {
                Ok(true) => {
                    let error = anyhow::anyhow!(
                        "new owned process reused the PID of a live tracked process"
                    );
                    return Err(cleanup_failed_process(error, process));
                }
                Ok(false) => {}
                Err(error) => return Err(cleanup_failed_process(error, process)),
            }
        }
        if self.descendants.contains_key(&identity) {
            let error = anyhow::anyhow!("new owned process duplicated a tracked process identity");
            return Err(cleanup_failed_process(error, process));
        }
        unsafe {
            CloseHandle(process.hThread);
        }
        self.descendants.insert(
            identity,
            TrackedProcess {
                handle: process.hProcess,
                identity,
            },
        );
        Ok(())
    }

    fn process_ids(&mut self) -> Result<BTreeSet<u32>> {
        self.capture_descendants()?;
        if !self.uncertain_pids.is_empty() {
            bail!(
                "owned descendant identity is uncertain; inaccessible={:?}",
                self.uncertain_pids
            );
        }
        const CAPACITY: usize = 4096;
        let header_words =
            size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>().div_ceil(size_of::<usize>());
        let mut storage = vec![0usize; header_words + CAPACITY - 1];
        let info = storage.as_mut_ptr() as *mut JOBOBJECT_BASIC_PROCESS_ID_LIST;
        let queried = unsafe {
            QueryInformationJobObject(
                self.handle,
                JobObjectBasicProcessIdList,
                info as *mut c_void,
                (storage.len() * size_of::<usize>()) as u32,
                null_mut(),
            )
        };
        if queried == 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to query owned Windows job processes");
        }
        let count = unsafe { (*info).NumberOfProcessIdsInList as usize };
        let assigned = unsafe { (*info).NumberOfAssignedProcesses as usize };
        if count != assigned {
            bail!("owned Windows job process list is truncated");
        }
        if count > CAPACITY {
            bail!("owned Windows job process list exceeds the safety limit");
        }
        let ids = unsafe { std::slice::from_raw_parts((*info).ProcessIdList.as_ptr(), count) };
        Ok(ids.iter().map(|pid| *pid as u32).collect())
    }

    fn capture_descendants(&mut self) -> Result<CaptureOutcome> {
        let capture = capture_descendant_handles(&mut self.descendants)?;
        self.uncertain_pids
            .extend(capture.inaccessible.iter().copied());
        Ok(capture)
    }

    fn terminate(mut self) -> Result<()> {
        let initial_capture_error = match self.capture_descendants() {
            Ok(_) if self.uncertain_pids.is_empty() => None,
            Ok(_) => Some(anyhow::anyhow!(
                "initial descendant identity is uncertain; inaccessible={:?}",
                self.uncertain_pids
            )),
            Err(error) => Some(error),
        };
        let terminated = unsafe { TerminateJobObject(self.handle, 0) };
        let terminate_error = if terminated == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        let descendant_error =
            terminate_descendant_lineage(&mut self.descendants, Duration::from_secs(10)).err();
        let cleanup_error = match (initial_capture_error, descendant_error) {
            (None, None) => None,
            (Some(error), None) => Some(error.context("initial descendant snapshot failed")),
            (None, Some(error)) => Some(error),
            (Some(initial), Some(cleanup)) => Some(anyhow::anyhow!(
                "initial descendant snapshot failed: {initial}; descendant cleanup failed: {cleanup}"
            )),
        };
        for process in self.descendants.values() {
            unsafe {
                WaitForSingleObject(process.handle, 5000);
            }
        }
        self.descendants.clear();
        unsafe {
            CloseHandle(self.handle);
        }
        self.handle = null_mut();
        match (terminate_error, cleanup_error) {
            (None, None) => Ok(()),
            (Some(error), None) => Err(error).context("failed to terminate owned Windows job"),
            (None, Some(error)) => Err(error),
            (Some(job), Some(descendants)) => Err(anyhow::anyhow!(
                "failed to terminate owned Windows job: {job}; descendant cleanup failed: {descendants}"
            )),
        }
    }
}

impl Drop for OwnedJob {
    fn drop(&mut self) {
        if self.handle.is_null() {
            return;
        }
        let _ = self.capture_descendants();
        unsafe {
            TerminateJobObject(self.handle, 0);
        }
        let _ = terminate_descendant_lineage(&mut self.descendants, Duration::from_secs(5));
        unsafe {
            CloseHandle(self.handle);
        }
        self.handle = null_mut();
    }
}

struct CaptureOutcome {
    added: usize,
    inaccessible: BTreeSet<u32>,
}

enum SnapshotProcess {
    Opened(TrackedProcess),
    Inaccessible,
    VanishedBeforeOpen,
    ReusedAfterSnapshot,
}

fn add_process_generation(
    generations: &mut BTreeMap<u32, Vec<ProcessGeneration>>,
    pid: u32,
    generation: ProcessGeneration,
) {
    let entries = generations.entry(pid).or_default();
    if !entries
        .iter()
        .any(|existing| existing.created_at == generation.created_at)
    {
        entries.push(generation);
    }
}

fn tracked_generation_is_parent_candidate(generation: ProcessGeneration) -> bool {
    generation.created_at != 0
        && generation
            .exited_at
            .is_none_or(|exited_at| exited_at >= generation.created_at)
}

fn descendant_entry_needs_retry(_parent_pid_known: bool, generation_matched: bool) -> bool {
    !generation_matched
}

fn open_snapshot_process(pid: u32, snapshot_at: u64) -> Result<SnapshotProcess> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE_ACCESS,
            0,
            pid,
        )
    };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(SnapshotProcess::VanishedBeforeOpen)
        } else {
            Ok(SnapshotProcess::Inaccessible)
        };
    }
    let process = TrackedProcess::from_handle(pid, handle)?;
    if process.identity.created_at > snapshot_at {
        drop(process);
        return Ok(SnapshotProcess::ReusedAfterSnapshot);
    }
    Ok(SnapshotProcess::Opened(process))
}

fn capture_descendant_handles(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
) -> Result<CaptureOutcome> {
    let snapshot = snapshot_process_entries()?;
    let mut generations = BTreeMap::<u32, Vec<ProcessGeneration>>::new();
    let mut existing_generations = BTreeMap::<u32, Vec<ProcessGeneration>>::new();
    for (identity, process) in descendants.iter() {
        let pid = identity.pid;
        let generation = process
            .generation()
            .with_context(|| format!("failed to inspect tracked process {pid}"))?;
        add_process_generation(&mut existing_generations, pid, generation);
        if tracked_generation_is_parent_candidate(generation) {
            add_process_generation(&mut generations, pid, generation);
        }
    }

    let mut pending = snapshot
        .entries
        .into_iter()
        .filter(|entry| entry.pid != 0 && entry.pid != entry.parent_pid)
        .collect::<Vec<_>>();
    let mut discovered = BTreeMap::<ProcessIdentity, TrackedProcess>::new();
    let mut added = 0;
    let mut inaccessible = BTreeSet::new();
    loop {
        let mut advanced = false;
        let mut remaining = Vec::new();
        for entry in pending {
            let parent_pid_known = generations.contains_key(&entry.parent_pid);
            if !parent_pid_known {
                if descendant_entry_needs_retry(false, false) {
                    remaining.push(entry);
                }
                continue;
            }

            let existing = existing_generations
                .get(&entry.pid)
                .and_then(|generations| {
                    generations.iter().copied().find(|generation| {
                        generation
                            .exited_at
                            .is_none_or(|exited_at| exited_at >= snapshot.captured_at)
                    })
                });
            let (generation, opened) = if let Some(generation) = existing {
                (generation, None)
            } else {
                match open_snapshot_process(entry.pid, snapshot.captured_at)? {
                    SnapshotProcess::Opened(process) => {
                        let generation = process.generation()?;
                        (generation, Some(process))
                    }
                    SnapshotProcess::Inaccessible => {
                        inaccessible.insert(entry.pid);
                        continue;
                    }
                    SnapshotProcess::VanishedBeforeOpen | SnapshotProcess::ReusedAfterSnapshot => {
                        inaccessible.insert(entry.pid);
                        continue;
                    }
                }
            };
            let belongs = generations.get(&entry.parent_pid).is_some_and(|parents| {
                parents
                    .iter()
                    .copied()
                    .any(|parent| parent.can_parent(generation.created_at, snapshot.captured_at))
            });
            if !belongs {
                drop(opened);
                if descendant_entry_needs_retry(true, false) {
                    remaining.push(entry);
                }
                continue;
            }

            add_process_generation(&mut generations, entry.pid, generation);
            if let Some(process) = opened {
                let already_tracked =
                    existing_generations
                        .get(&entry.pid)
                        .is_some_and(|existing| {
                            existing
                                .iter()
                                .any(|existing| existing.created_at == generation.created_at)
                        });
                if already_tracked {
                    drop(process);
                } else {
                    drop(discovered.insert(process.identity, process));
                    added += 1;
                }
            }
            advanced = true;
        }
        if !advanced {
            break;
        }
        pending = remaining;
    }

    if inaccessible.is_empty() {
        let completed = descendants
            .iter()
            .map(|(identity, process)| {
                process_handle_is_running(process.handle)
                    .map(|running| (!running).then_some(*identity))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        for identity in completed {
            drop(descendants.remove(&identity));
        }
    }
    for (identity, process) in discovered {
        if !process_handle_is_running(process.handle)? {
            drop(process);
            continue;
        }
        if descendants.contains_key(&identity) {
            bail!("discovered descendant duplicated a tracked process identity");
        }
        descendants.insert(identity, process);
    }
    Ok(CaptureOutcome {
        added,
        inaccessible,
    })
}

fn terminate_descendant_lineage(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
    timeout: Duration,
) -> Result<()> {
    terminate_descendant_lineage_with_capture(descendants, timeout, capture_descendant_handles)
}

fn terminate_descendant_lineage_with_capture<F>(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
    timeout: Duration,
    mut capture: F,
) -> Result<()>
where
    F: FnMut(&mut BTreeMap<ProcessIdentity, TrackedProcess>) -> Result<CaptureOutcome>,
{
    let result = (|| {
        let deadline = Instant::now() + timeout;
        let mut stable_empty_passes = 0;
        let mut first_capture_error = None;
        let mut first_inaccessible = BTreeSet::new();
        loop {
            let capture = capture(descendants);
            if let Err(error) = &capture
                && first_capture_error.is_none()
            {
                first_capture_error = Some(error.to_string());
            }
            if let Ok(capture) = &capture {
                first_inaccessible.extend(capture.inaccessible.iter().copied());
            }
            for process in descendants.values() {
                if process_handle_is_running(process.handle).unwrap_or(true) {
                    unsafe {
                        TerminateProcess(process.handle, 0);
                    }
                }
            }
            thread::sleep(Duration::from_millis(50));
            let live = descendants
                .iter()
                .filter_map(|(identity, process)| {
                    process_handle_is_running(process.handle)
                        .unwrap_or(true)
                        .then_some(identity.pid)
                })
                .collect::<BTreeSet<_>>();
            match &capture {
                Ok(capture)
                    if live.is_empty() && capture.added == 0 && capture.inaccessible.is_empty() =>
                {
                    stable_empty_passes += 1;
                    if stable_empty_passes >= DESCENDANT_QUIESCENCE_PASSES {
                        return match (first_capture_error, first_inaccessible.is_empty()) {
                            (None, true) => Ok(()),
                            (Some(error), true) => {
                                bail!("process snapshot failed during descendant cleanup: {error}")
                            }
                            (None, false) => bail!(
                                "descendant identity remained uncertain during cleanup; inaccessible={first_inaccessible:?}"
                            ),
                            (Some(error), false) => bail!(
                                "process snapshot failed during descendant cleanup: {error}; inaccessible={first_inaccessible:?}"
                            ),
                        };
                    }
                }
                _ => stable_empty_passes = 0,
            }
            if Instant::now() >= deadline {
                match capture {
                    Ok(capture) => bail!(
                        "owned descendant cleanup timed out; live={live:?}; inaccessible={:?}",
                        capture.inaccessible
                    ),
                    Err(error) => bail!(
                        "owned descendant cleanup timed out after process snapshot failure: {error}; live={live:?}"
                    ),
                }
            }
        }
    })();
    descendants.clear();
    result
}

fn cleanup_failed_process(error: anyhow::Error, process: PROCESS_INFORMATION) -> anyhow::Error {
    let terminated = unsafe { TerminateProcess(process.hProcess, 1) };
    let terminate_error = (terminated == 0).then(std::io::Error::last_os_error);
    let waited = unsafe { WaitForSingleObject(process.hProcess, 5000) };
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    match (terminate_error, waited == WAIT_OBJECT_0) {
        (None, true) => error,
        (Some(cleanup), _) => {
            anyhow::anyhow!("{error}; failed to terminate rejected process: {cleanup}")
        }
        (None, false) => anyhow::anyhow!(
            "{error}; rejected process did not terminate before cleanup deadline (wait={waited})"
        ),
    }
}

fn process_handle_is_running(handle: HANDLE) -> Result<bool> {
    match unsafe { WaitForSingleObject(handle, 0) } {
        WAIT_OBJECT_0 => Ok(false),
        WAIT_TIMEOUT => Ok(true),
        status => bail!("failed to query tracked process state (wait={status})"),
    }
}

fn process_times(handle: HANDLE) -> Result<(u64, u64)> {
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let read =
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    if read == 0 {
        return Err(std::io::Error::last_os_error()).context("failed to query process times");
    }
    Ok((filetime_value(created), filetime_value(exited)))
}

fn filetime_value(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

fn current_filetime() -> u64 {
    const WINDOWS_EPOCH_OFFSET_SECONDS: u64 = 11_644_473_600;
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (elapsed.as_secs() + WINDOWS_EPOCH_OFFSET_SECONDS) * 10_000_000
        + u64::from(elapsed.subsec_nanos() / 100)
}

struct ProcessSnapshotEntry {
    pid: u32,
    parent_pid: u32,
    executable: OsString,
}

struct ProcessSnapshot {
    captured_at: u64,
    entries: Vec<ProcessSnapshotEntry>,
}

fn snapshot_process_entries() -> Result<ProcessSnapshot> {
    let captured_at = current_filetime();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error())
            .context("failed to snapshot Windows processes");
    }
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut entries = Vec::new();
    let mut available = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while available {
        let length = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        let executable = OsString::from_wide(&entry.szExeFile[..length]);
        entries.push(ProcessSnapshotEntry {
            pid: entry.th32ProcessID,
            parent_pid: entry.th32ParentProcessID,
            executable,
        });
        available = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    let last_error = std::io::Error::last_os_error();
    unsafe {
        CloseHandle(snapshot);
    }
    if last_error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) {
        return Err(last_error).context("failed while enumerating Windows processes");
    }
    Ok(ProcessSnapshot {
        captured_at,
        entries,
    })
}

fn snapshot_processes_named(name: &str) -> Result<BTreeSet<u32>> {
    Ok(snapshot_process_entries()?
        .entries
        .into_iter()
        .filter(|entry| {
            entry
                .executable
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
        .map(|entry| entry.pid)
        .collect())
}

#[cfg(test)]
fn descendant_process_ids(entries: &[(u32, u32)], roots: &BTreeSet<u32>) -> BTreeSet<u32> {
    let mut descendants = roots.clone();
    loop {
        let previous_len = descendants.len();
        for &(pid, parent_pid) in entries {
            if pid != 0 && pid != parent_pid && descendants.contains(&parent_pid) {
                descendants.insert(pid);
            }
        }
        if descendants.len() == previous_len {
            return descendants;
        }
    }
}

fn process_image_path(pid: u32) -> Option<PathBuf> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let result = process_image_path_from_handle(handle).ok();
    unsafe {
        CloseHandle(handle);
    }
    result
}

fn reject_reparse_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                bail!(
                    "isolated runtime path contains a reparse point: {}",
                    ancestor.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to inspect isolated runtime ancestor {}",
                        ancestor.display()
                    )
                });
            }
        }
    }
    Ok(())
}

fn process_image_path_from_handle(handle: HANDLE) -> Result<PathBuf> {
    let mut buffer = vec![0_u16; 32768];
    let mut length = buffer.len() as u32;
    let queried =
        unsafe { QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) };
    if queried == 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to read created process image");
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}

fn validate_created_official_process(executable: &Path, handle: HANDLE) -> Result<()> {
    let observed = process_image_path_from_handle(handle)?;
    if !same_canonical_windows_path(executable, &observed)? {
        bail!(
            "created process image {} does not match official executable {}",
            observed.display(),
            executable.display()
        );
    }
    let family = process_package_family(handle)?;
    if family != OFFICIAL_PACKAGE_FAMILY {
        bail!("created process does not belong to the official OpenAI.Codex package family");
    }
    Ok(())
}

fn same_canonical_windows_path(left: &Path, right: &Path) -> Result<bool> {
    let left = fs::canonicalize(left)
        .with_context(|| format!("failed to resolve official executable {}", left.display()))?;
    let right = fs::canonicalize(right).with_context(|| {
        format!(
            "failed to resolve created process image {}",
            right.display()
        )
    })?;
    Ok(left
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy()))
}

fn process_package_family(handle: HANDLE) -> Result<String> {
    let mut length = 0_u32;
    let status = unsafe { GetPackageFamilyName(handle, &mut length, null_mut()) };
    if status != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32))
            .context("created process has no readable Windows package family");
    }
    let mut buffer = vec![0_u16; length as usize];
    let status = unsafe { GetPackageFamilyName(handle, &mut length, buffer.as_mut_ptr()) };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32))
            .context("failed to read created process package family");
    }
    let content_length = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(&buffer[..content_length])
        .context("created process package family is not valid UTF-16")
}

fn loopback_listener_pid(port: u16) -> Result<u32> {
    let mut required_bytes = 0_u32;
    let status = unsafe {
        GetExtendedTcpTable(
            null_mut(),
            &mut required_bytes,
            0,
            u32::from(AF_INET),
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER || required_bytes == 0 {
        return Err(std::io::Error::from_raw_os_error(status as i32))
            .context("failed to size the Windows TCP listener table");
    }

    for _ in 0..4 {
        let mut storage = vec![0_usize; (required_bytes as usize).div_ceil(size_of::<usize>())];
        let mut available_bytes = (storage.len() * size_of::<usize>()) as u32;
        let status = unsafe {
            GetExtendedTcpTable(
                storage.as_mut_ptr() as *mut c_void,
                &mut available_bytes,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER {
            if available_bytes == 0 {
                bail!("Windows TCP listener table reported an empty resize");
            }
            required_bytes = available_bytes;
            continue;
        }
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32))
                .context("failed to read the Windows TCP listener table");
        }

        let table = storage.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
        let count = unsafe { (*table).dwNumEntries as usize };
        let table_bytes = size_of::<u32>() + count * size_of::<MIB_TCPROW_OWNER_PID>();
        if table_bytes > available_bytes as usize {
            bail!("Windows TCP listener table is truncated");
        }
        let rows = unsafe { std::slice::from_raw_parts((*table).table.as_ptr(), count) };
        let owners: BTreeSet<u32> = rows
            .iter()
            .filter(|row| {
                row.dwState == MIB_TCP_STATE_LISTEN as u32
                    && u16::from_be(row.dwLocalPort as u16) == port
                    && u32::from_be(row.dwLocalAddr) == u32::from(std::net::Ipv4Addr::LOCALHOST)
            })
            .map(|row| row.dwOwningPid)
            .collect();
        return match owners.len() {
            1 => Ok(*owners.first().unwrap()),
            0 => bail!("isolated CDP port has no loopback listener owner"),
            _ => bail!("isolated CDP port has multiple loopback listener owners"),
        };
    }
    bail!("Windows TCP listener table kept growing during ownership lookup")
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn windows_command_line(executable: &OsStr, arguments: &[OsString]) -> Vec<u16> {
    let mut rendered = quote_windows_argument(executable);
    for argument in arguments {
        rendered.push(' ');
        rendered.push_str(&quote_windows_argument(argument));
    }
    wide_null(OsStr::new(&rendered))
}

fn quote_windows_argument(argument: &OsStr) -> String {
    let value = argument.to_string_lossy();
    if !value.is_empty()
        && !value
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return value.into_owned();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for character in value.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(character);
            }
        }
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

fn windows_environment_block(overrides: &[(OsString, OsString)]) -> Result<Vec<u16>> {
    let mut environment = BTreeMap::<String, (OsString, OsString)>::new();
    for (key, value) in std::env::vars_os() {
        environment.insert(key.to_string_lossy().to_uppercase(), (key, value));
    }
    for (key, value) in overrides {
        if key.is_empty()
            || key.to_string_lossy().contains(['=', '\0'])
            || value.to_string_lossy().contains('\0')
        {
            bail!("isolated process environment override is invalid");
        }
        environment.insert(
            key.to_string_lossy().to_uppercase(),
            (key.clone(), value.clone()),
        );
    }
    let mut block = Vec::new();
    for (_, (key, value)) in environment {
        block.extend(key.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, process::Command};
    use windows_sys::Win32::{
        Foundation::STILL_ACTIVE,
        System::Threading::{GetExitCodeProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    fn powershell_path() -> PathBuf {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe")
    }

    fn process_is_alive(pid: u32) -> bool {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let read = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
        unsafe {
            CloseHandle(handle);
        }
        read && exit_code == STILL_ACTIVE as u32
    }

    #[test]
    fn owned_root_cleanup_waits_for_a_late_child_file_release() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("instance");
        fs::create_dir(&root).unwrap();
        let held_path = root.join("state.lock");
        fs::write(&held_path, b"held").unwrap();
        let held = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&held_path)
            .unwrap();
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(2_250));
            drop(held);
        });
        let mut runtime = WindowsDirectRuntime::new(root.clone(), None).unwrap();
        runtime.owns_root = true;

        let started = Instant::now();
        runtime.remove_owned_root().unwrap();

        release.join().unwrap();
        assert!(started.elapsed() >= Duration::from_millis(2_000));
        assert!(!root.exists());
    }

    #[test]
    fn escaped_process_cleanup_tracks_the_full_descendant_tree() {
        let entries = [(20, 10), (30, 20), (40, 30), (50, 999), (60, 50)];

        assert_eq!(
            descendant_process_ids(&entries, &BTreeSet::from([10])),
            BTreeSet::from([10, 20, 30, 40])
        );
    }

    #[test]
    fn process_generation_accepts_a_child_created_before_parent_exit() {
        let parent = ProcessGeneration {
            created_at: 100,
            exited_at: Some(300),
        };

        assert!(parent.can_parent(200, 400));
    }

    #[test]
    fn process_generation_rejects_a_child_created_after_parent_exit() {
        let parent = ProcessGeneration {
            created_at: 100,
            exited_at: Some(300),
        };

        assert!(!parent.can_parent(301, 400));
    }

    #[test]
    fn process_generation_rejects_a_process_created_after_the_snapshot() {
        let parent = ProcessGeneration {
            created_at: 100,
            exited_at: None,
        };

        assert!(!parent.can_parent(401, 400));
    }

    #[test]
    fn completed_generation_remains_a_parent_candidate_after_pid_reuse() {
        let generation = ProcessGeneration {
            created_at: 100,
            exited_at: Some(300),
        };

        assert!(tracked_generation_is_parent_candidate(generation));
    }

    #[test]
    fn generation_mismatch_retries_for_a_later_parent_generation() {
        assert!(descendant_entry_needs_retry(true, false));
    }

    #[test]
    fn descendant_cleanup_terminates_known_handles_when_snapshot_fails() {
        let mut child = Command::new(powershell_path())
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .spawn()
            .unwrap();
        let pid = child.id();
        let handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE_ACCESS,
                0,
                pid,
            )
        };
        assert!(!handle.is_null());
        let process = TrackedProcess::from_handle(pid, handle).unwrap();
        let mut descendants = BTreeMap::from([(process.identity, process)]);

        let result = terminate_descendant_lineage_with_capture(
            &mut descendants,
            Duration::from_millis(150),
            |_| Err(anyhow::anyhow!("injected process snapshot failure")),
        );

        let deadline = Instant::now() + Duration::from_secs(1);
        while process_is_alive(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let terminated = !process_is_alive(pid);
        if terminated {
            let _ = child.wait();
        } else {
            let _ = child.kill();
            let _ = child.wait();
        }

        assert!(result.is_err());
        assert!(terminated);
    }

    #[test]
    fn descendant_cleanup_reports_a_transient_snapshot_failure() {
        let mut descendants = BTreeMap::new();
        let mut captures = 0;

        let result = terminate_descendant_lineage_with_capture(
            &mut descendants,
            Duration::from_secs(3),
            |_| {
                captures += 1;
                if captures == 1 {
                    Err(anyhow::anyhow!("injected transient snapshot failure"))
                } else {
                    Ok(CaptureOutcome {
                        added: 0,
                        inaccessible: BTreeSet::new(),
                    })
                }
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn descendant_cleanup_reports_a_transient_inaccessible_process() {
        let mut descendants = BTreeMap::new();
        let mut captures = 0;

        let result = terminate_descendant_lineage_with_capture(
            &mut descendants,
            Duration::from_secs(3),
            |_| {
                captures += 1;
                Ok(CaptureOutcome {
                    added: 0,
                    inaccessible: if captures == 1 {
                        BTreeSet::from([42])
                    } else {
                        BTreeSet::new()
                    },
                })
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn owned_job_terminates_a_spawned_process() {
        let executable = powershell_path();
        let mut job = OwnedJob::create().unwrap();
        job.launch(
            &executable,
            &[
                OsString::from("-NoProfile"),
                OsString::from("-NonInteractive"),
                OsString::from("-Command"),
                OsString::from("Start-Sleep -Seconds 30"),
            ],
            &[],
            false,
        )
        .unwrap();

        let owned = job
            .descendants
            .keys()
            .map(|identity| identity.pid)
            .collect::<BTreeSet<_>>();
        assert_eq!(owned.len(), 1);
        assert!(owned.iter().all(|pid| process_is_alive(*pid)));

        job.terminate().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while owned.iter().any(|pid| process_is_alive(*pid)) && std::time::Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(owned.iter().all(|pid| !process_is_alive(*pid)));
    }
}
