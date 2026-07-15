use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString, c_void},
    fs,
    mem::{size_of, size_of_val},
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
use windows_sys::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessCommandLineInformation,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES,
        FILETIME, HANDLE, INVALID_HANDLE_VALUE, LocalFree, STILL_ACTIVE, UNICODE_STRING,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
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
            GetCurrentProcessId, GetExitCodeProcess, GetProcessTimes, OpenProcess,
            PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
            QueryFullProcessImageNameW, ResumeThread, STARTUPINFOW, TerminateProcess,
            WaitForSingleObject,
        },
    },
    UI::Shell::CommandLineToArgvW,
};

const OFFICIAL_PACKAGE_FAMILY: &str = "OpenAI.Codex_2p2nqsd0c76g0";
const PROCESS_SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;
const DESCENDANT_QUIESCENCE_DURATION: Duration = Duration::from_secs(5);
const DROP_DESCENDANT_TIMEOUT: Duration = Duration::from_secs(10);

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
            if Instant::now() >= deadline {
                bail!("owned-root cleanup reached its deadline before removal");
            }
            terminate_owned_root_references_once(&self.root, deadline)?;
            if Instant::now() >= deadline {
                bail!("owned-root cleanup reached its deadline before removal");
            }
            match fs::remove_dir_all(&self.root) {
                Ok(()) => {
                    self.owns_root = false;
                    if Instant::now() >= deadline {
                        bail!("owned-root removal completed after its deadline");
                    }
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.owns_root = false;
                    if Instant::now() >= deadline {
                        bail!("owned-root removal completed after its deadline");
                    }
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
            OwnedJob::create(self.root.clone())
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
                .is_none_or(|exited_at| child_created_at < exited_at)
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
    owned_root: PathBuf,
    descendants: BTreeMap<ProcessIdentity, TrackedProcess>,
    uncertain_pids: BTreeSet<u32>,
    uncertain_details: BTreeMap<u32, String>,
    lineage: LineageObservationState,
}

impl OwnedJob {
    fn create(owned_root: PathBuf) -> Result<Self> {
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
            owned_root,
            descendants: BTreeMap::new(),
            uncertain_pids: BTreeSet::new(),
            uncertain_details: BTreeMap::new(),
            lineage: LineageObservationState::default(),
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
        self.capture_descendants(false)?;
        if !self.uncertain_pids.is_empty() {
            bail!(
                "owned descendant identity is uncertain; inaccessible={:?}; details={:?}",
                self.uncertain_pids,
                self.uncertain_details
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

    fn capture_descendants(&mut self, scan_owned_root: bool) -> Result<CaptureOutcome> {
        let input = self
            .lineage
            .capture_input(Instant::now(), DESCENDANT_QUIESCENCE_DURATION);
        let capture = capture_descendant_handles(
            &mut self.descendants,
            &input.anchors,
            &input.members,
            &input.edges,
            input.previous_snapshot_at,
            scan_owned_root.then_some(self.owned_root.as_path()),
        )?;
        let observed_at = Instant::now();
        self.lineage
            .observe_capture(&capture, observed_at, DESCENDANT_QUIESCENCE_DURATION);
        self.uncertain_pids
            .extend(capture.inaccessible.iter().copied());
        self.uncertain_details
            .extend(capture.inaccessible_details.clone());
        Ok(capture)
    }

    fn terminate(mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let initial_capture_error = match self.capture_descendants(true) {
            Ok(_) if self.uncertain_pids.is_empty() => None,
            Ok(_) => Some(anyhow::anyhow!(
                "initial descendant identity is uncertain; inaccessible={:?}; details={:?}",
                self.uncertain_pids,
                self.uncertain_details
            )),
            Err(error) => Some(error),
        };
        let terminated = unsafe { TerminateJobObject(self.handle, 0) };
        let terminate_error = if terminated == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        let descendant_error = terminate_descendant_lineage_until(
            &mut self.descendants,
            &mut self.lineage,
            &self.owned_root,
            deadline,
        )
        .err();
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
        let deadline = Instant::now() + DROP_DESCENDANT_TIMEOUT;
        let _ = self.capture_descendants(true);
        unsafe {
            TerminateJobObject(self.handle, 0);
        }
        let _ = terminate_descendant_lineage_until(
            &mut self.descendants,
            &mut self.lineage,
            &self.owned_root,
            deadline,
        );
        unsafe {
            CloseHandle(self.handle);
        }
        self.handle = null_mut();
    }
}

struct CaptureOutcome {
    added: usize,
    inaccessible: BTreeSet<u32>,
    inaccessible_details: BTreeMap<u32, String>,
    lineage_anchors: BTreeSet<u32>,
    uncertain_lineage: BTreeSet<u32>,
    observed_edges: Vec<(u32, u32)>,
    snapshot_at: Option<u64>,
}

#[cfg(test)]
impl CaptureOutcome {
    fn empty() -> Self {
        Self {
            added: 0,
            inaccessible: BTreeSet::new(),
            inaccessible_details: BTreeMap::new(),
            lineage_anchors: BTreeSet::new(),
            uncertain_lineage: BTreeSet::new(),
            observed_edges: Vec::new(),
            snapshot_at: None,
        }
    }
}

#[derive(Default)]
struct ObservedLineageEdges {
    edges: BTreeMap<(u32, u32), Instant>,
}

impl ObservedLineageEdges {
    fn observe(&mut self, edges: &[(u32, u32)], observed_at: Instant) {
        for &(pid, parent_pid) in edges {
            if pid != 0 && pid != parent_pid {
                self.edges.insert((pid, parent_pid), observed_at);
            }
        }
    }

    fn prune(&mut self, now: Instant, duration: Duration) {
        self.edges
            .retain(|_, observed_at| now.saturating_duration_since(*observed_at) < duration);
    }

    fn pairs(&self) -> Vec<(u32, u32)> {
        self.edges.keys().copied().collect()
    }
}

#[derive(Default)]
struct LineageObservationState {
    anchors: BTreeMap<u32, Instant>,
    members: BTreeSet<u32>,
    edges: ObservedLineageEdges,
    last_snapshot_at: Option<u64>,
}

struct LineageCaptureInput {
    anchors: BTreeSet<u32>,
    members: BTreeSet<u32>,
    edges: Vec<(u32, u32)>,
    previous_snapshot_at: Option<u64>,
}

impl LineageObservationState {
    fn active_anchors(&self) -> BTreeSet<u32> {
        self.anchors.keys().copied().collect()
    }

    fn capture_input(&mut self, now: Instant, duration: Duration) -> LineageCaptureInput {
        self.prune(now, duration);
        LineageCaptureInput {
            anchors: self.active_anchors(),
            members: self.members.clone(),
            edges: self.edges.pairs(),
            previous_snapshot_at: self.last_snapshot_at,
        }
    }

    fn observe_capture(
        &mut self,
        capture: &CaptureOutcome,
        observed_at: Instant,
        duration: Duration,
    ) {
        self.edges.observe(&capture.observed_edges, observed_at);
        if let Some(snapshot_at) = capture.snapshot_at {
            self.last_snapshot_at = Some(snapshot_at);
        }
        for pid in &capture.lineage_anchors {
            self.anchors.entry(*pid).or_insert(observed_at);
        }
        for pid in &capture.uncertain_lineage {
            self.anchors.entry(*pid).or_insert(observed_at);
            self.members.insert(*pid);
        }
        if !capture.uncertain_lineage.is_empty() {
            for first_observed in self.anchors.values_mut() {
                *first_observed = observed_at;
            }
        }
        self.prune(observed_at, duration);
    }

    fn is_empty(&self) -> bool {
        self.anchors.is_empty() && self.members.is_empty()
    }

    fn prune(&mut self, now: Instant, duration: Duration) {
        self.anchors
            .retain(|_, observed_at| now.saturating_duration_since(*observed_at) < duration);
        self.members.retain(|pid| self.anchors.contains_key(pid));
        self.edges.prune(now, duration);
    }

    fn clear(&mut self) {
        self.anchors.clear();
        self.members.clear();
        self.edges.edges.clear();
        self.last_snapshot_at = None;
    }
}

enum SnapshotProcess {
    Opened(TrackedProcess),
    Inaccessible { error_code: Option<i32> },
    VanishedBeforeOpen,
    ReusedAfterSnapshot,
}

fn snapshot_process_requires_uncertainty(process: &SnapshotProcess) -> bool {
    matches!(process, SnapshotProcess::Inaccessible { .. })
}

fn snapshot_process_requires_lineage_anchor(process: &SnapshotProcess) -> bool {
    matches!(
        process,
        SnapshotProcess::VanishedBeforeOpen | SnapshotProcess::ReusedAfterSnapshot
    )
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

fn unresolved_descendant_requires_lineage_anchor(parent_pid_known: bool) -> bool {
    parent_pid_known
}

fn snapshot_generation_precedes_capture(created_at: u64, snapshot_at: u64) -> bool {
    created_at < snapshot_at
}

fn validate_snapshot_clock(snapshot_at: u64, observed_at: u64) -> Result<()> {
    if observed_at < snapshot_at {
        bail!("system clock moved backwards during the process snapshot");
    }
    Ok(())
}

fn validate_snapshot_progress(previous_snapshot_at: Option<u64>, snapshot_at: u64) -> Result<()> {
    if previous_snapshot_at.is_some_and(|previous| snapshot_at < previous) {
        bail!("system clock moved backwards between process snapshots");
    }
    Ok(())
}

fn classify_snapshot_open_failure(
    error_code: Option<i32>,
    process_still_present: bool,
) -> SnapshotProcess {
    if !process_still_present {
        SnapshotProcess::VanishedBeforeOpen
    } else if error_code == Some(ERROR_INVALID_PARAMETER as i32) {
        SnapshotProcess::ReusedAfterSnapshot
    } else {
        SnapshotProcess::Inaccessible { error_code }
    }
}

fn open_snapshot_process(
    pid: u32,
    snapshot_at: u64,
    observed_edges: &mut Vec<(u32, u32)>,
    latest_snapshot_at: &mut u64,
) -> Result<SnapshotProcess> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE_ACCESS,
            0,
            pid,
        )
    };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        let recheck = snapshot_process_entries()?;
        validate_snapshot_progress(Some(*latest_snapshot_at), recheck.captured_at)?;
        *latest_snapshot_at = recheck.captured_at;
        observed_edges.extend(
            recheck
                .entries
                .iter()
                .map(|entry| (entry.pid, entry.parent_pid)),
        );
        let process_still_present = recheck.entries.iter().any(|entry| entry.pid == pid);
        return Ok(classify_snapshot_open_failure(
            error.raw_os_error(),
            process_still_present,
        ));
    }
    let process = TrackedProcess::from_handle(pid, handle)?;
    validate_snapshot_clock(*latest_snapshot_at, current_filetime())?;
    if !snapshot_generation_precedes_capture(process.identity.created_at, snapshot_at) {
        drop(process);
        return Ok(SnapshotProcess::ReusedAfterSnapshot);
    }
    Ok(SnapshotProcess::Opened(process))
}

fn capture_descendant_handles(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
    lineage_anchors: &BTreeSet<u32>,
    lineage_members: &BTreeSet<u32>,
    historical_edges: &[(u32, u32)],
    previous_snapshot_at: Option<u64>,
    owned_root: Option<&Path>,
) -> Result<CaptureOutcome> {
    let snapshot = snapshot_process_entries()?;
    validate_snapshot_progress(previous_snapshot_at, snapshot.captured_at)?;
    let snapshot_at = snapshot.captured_at;
    let mut latest_snapshot_at = snapshot_at;
    let mut observed_edges = snapshot
        .entries
        .iter()
        .map(|entry| (entry.pid, entry.parent_pid))
        .collect::<Vec<_>>();
    let mut lineage_edges = historical_edges.to_vec();
    lineage_edges.extend(observed_edges.iter().copied());
    let active_uncertain_lineage = lineage_process_ids(&lineage_edges, lineage_anchors);
    let root_added = match owned_root {
        Some(root) => capture_owned_root_references(&snapshot.entries, root, descendants)?,
        None => 0,
    };
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
        .filter(|entry| {
            entry.pid != 0
                && entry.pid != entry.parent_pid
                && !active_uncertain_lineage.contains(&entry.pid)
        })
        .collect::<Vec<_>>();
    let mut discovered = BTreeMap::<ProcessIdentity, TrackedProcess>::new();
    let mut added = root_added;
    let mut inaccessible = BTreeSet::new();
    let mut inaccessible_details = BTreeMap::new();
    let mut new_lineage_anchors = BTreeSet::new();
    let mut visible_lineage_anchors = BTreeSet::new();
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
                            .is_none_or(|exited_at| exited_at >= snapshot_at)
                    })
                });
            let (generation, opened) = if let Some(generation) = existing {
                (generation, None)
            } else {
                match open_snapshot_process(
                    entry.pid,
                    snapshot_at,
                    &mut observed_edges,
                    &mut latest_snapshot_at,
                )? {
                    SnapshotProcess::Opened(process) => {
                        let generation = process.generation()?;
                        (generation, Some(process))
                    }
                    outcome => {
                        if snapshot_process_requires_uncertainty(&outcome) {
                            inaccessible.insert(entry.pid);
                            if let SnapshotProcess::Inaccessible { error_code } = &outcome {
                                inaccessible_details.insert(
                                    entry.pid,
                                    format!(
                                        "{} (os_error={error_code:?})",
                                        entry.executable.to_string_lossy()
                                    ),
                                );
                            }
                        }
                        if snapshot_process_requires_lineage_anchor(&outcome) {
                            new_lineage_anchors.insert(entry.pid);
                        }
                        continue;
                    }
                }
            };
            let belongs = generations.get(&entry.parent_pid).is_some_and(|parents| {
                parents
                    .iter()
                    .copied()
                    .any(|parent| parent.can_parent(generation.created_at, snapshot_at))
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
            for entry in remaining {
                if unresolved_descendant_requires_lineage_anchor(
                    generations.contains_key(&entry.parent_pid),
                ) {
                    new_lineage_anchors.insert(entry.pid);
                    visible_lineage_anchors.insert(entry.pid);
                }
            }
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
    let mut complete_lineage_edges = historical_edges.to_vec();
    complete_lineage_edges.extend(observed_edges.iter().copied());
    let uncertain_roots = lineage_anchors
        .iter()
        .chain(new_lineage_anchors.iter())
        .chain(inaccessible.iter())
        .copied()
        .collect::<BTreeSet<_>>();
    let mut uncertain_lineage = uncertain_lineage_process_ids(
        &complete_lineage_edges,
        &observed_edges,
        &uncertain_roots,
        lineage_members,
    );
    uncertain_lineage.extend(visible_lineage_anchors);
    Ok(CaptureOutcome {
        added,
        inaccessible,
        inaccessible_details,
        lineage_anchors: new_lineage_anchors,
        uncertain_lineage,
        observed_edges,
        snapshot_at: Some(latest_snapshot_at),
    })
}

struct DescendantQuiescence {
    duration: Duration,
    stable_empty_since: Option<Instant>,
}

impl DescendantQuiescence {
    fn new(duration: Duration) -> Self {
        Self {
            duration,
            stable_empty_since: None,
        }
    }

    fn observe(&mut self, now: Instant, empty: bool) -> bool {
        if !empty {
            self.stable_empty_since = None;
            return false;
        }
        let stable_since = self.stable_empty_since.get_or_insert(now);
        now.saturating_duration_since(*stable_since) >= self.duration
    }
}

fn terminate_descendant_lineage_until(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
    lineage: &mut LineageObservationState,
    owned_root: &Path,
    deadline: Instant,
) -> Result<()> {
    terminate_descendant_lineage_with_policy_until(
        descendants,
        lineage,
        deadline,
        DESCENDANT_QUIESCENCE_DURATION,
        |descendants, anchors, members, edges, previous_snapshot_at| {
            capture_descendant_handles(
                descendants,
                anchors,
                members,
                edges,
                previous_snapshot_at,
                Some(owned_root),
            )
        },
    )
}

#[cfg(test)]
fn terminate_descendant_lineage_with_capture<F>(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
    timeout: Duration,
    mut capture: F,
) -> Result<()>
where
    F: FnMut(&mut BTreeMap<ProcessIdentity, TrackedProcess>) -> Result<CaptureOutcome>,
{
    let mut lineage = LineageObservationState::default();
    terminate_descendant_lineage_with_policy(
        descendants,
        &mut lineage,
        timeout,
        DESCENDANT_QUIESCENCE_DURATION,
        |descendants, _, _, _, _| capture(descendants),
    )
}

#[cfg(test)]
fn terminate_descendant_lineage_with_policy<F>(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
    lineage: &mut LineageObservationState,
    timeout: Duration,
    quiescence_duration: Duration,
    capture: F,
) -> Result<()>
where
    F: FnMut(
        &mut BTreeMap<ProcessIdentity, TrackedProcess>,
        &BTreeSet<u32>,
        &BTreeSet<u32>,
        &[(u32, u32)],
        Option<u64>,
    ) -> Result<CaptureOutcome>,
{
    let deadline = Instant::now() + timeout;
    terminate_descendant_lineage_with_policy_until(
        descendants,
        lineage,
        deadline,
        quiescence_duration,
        capture,
    )
}

fn terminate_descendant_lineage_with_policy_until<F>(
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
    lineage: &mut LineageObservationState,
    deadline: Instant,
    quiescence_duration: Duration,
    mut capture: F,
) -> Result<()>
where
    F: FnMut(
        &mut BTreeMap<ProcessIdentity, TrackedProcess>,
        &BTreeSet<u32>,
        &BTreeSet<u32>,
        &[(u32, u32)],
        Option<u64>,
    ) -> Result<CaptureOutcome>,
{
    let result = (|| {
        let mut quiescence = DescendantQuiescence::new(quiescence_duration);
        let mut first_capture_error = None;
        let mut first_inaccessible = BTreeSet::new();
        let mut first_inaccessible_details = BTreeMap::new();
        loop {
            if Instant::now() >= deadline {
                bail!("owned descendant cleanup timed out before process capture");
            }
            let input = lineage.capture_input(Instant::now(), quiescence_duration);
            let capture = capture(
                descendants,
                &input.anchors,
                &input.members,
                &input.edges,
                input.previous_snapshot_at,
            );
            let captured_at = Instant::now();
            if let Err(error) = &capture
                && first_capture_error.is_none()
            {
                first_capture_error = Some(error.to_string());
            }
            if let Ok(capture) = &capture {
                lineage.observe_capture(capture, captured_at, quiescence_duration);
                first_inaccessible.extend(capture.inaccessible.iter().copied());
                first_inaccessible_details.extend(capture.inaccessible_details.clone());
            }
            for process in descendants.values() {
                if process_handle_is_running(process.handle).unwrap_or(true) {
                    unsafe {
                        TerminateProcess(process.handle, 0);
                    }
                }
            }
            let now = Instant::now();
            if now < deadline {
                thread::sleep(Duration::from_millis(50).min(deadline - now));
            }
            let live = descendants
                .iter()
                .filter_map(|(identity, process)| {
                    process_handle_is_running(process.handle)
                        .unwrap_or(true)
                        .then_some(identity.pid)
                })
                .collect::<BTreeSet<_>>();
            let observed_at = Instant::now();
            if observed_at >= deadline {
                let anchors = lineage.active_anchors();
                match &capture {
                    Ok(capture) => bail!(
                        "owned descendant cleanup timed out; live={live:?}; inaccessible={:?}; inaccessible_details={:?}; uncertain_lineage={:?}; lineage_anchors={anchors:?}",
                        capture.inaccessible,
                        capture.inaccessible_details,
                        capture.uncertain_lineage
                    ),
                    Err(error) => bail!(
                        "owned descendant cleanup timed out after process snapshot failure: {error}; live={live:?}; lineage_anchors={anchors:?}"
                    ),
                }
            }
            lineage.prune(observed_at, quiescence_duration);
            let capture_is_empty = matches!(
                &capture,
                Ok(capture)
                    if live.is_empty()
                        && capture.added == 0
                        && capture.inaccessible.is_empty()
                        && capture.lineage_anchors.is_empty()
                        && capture.uncertain_lineage.is_empty()
            );
            if quiescence.observe(observed_at, capture_is_empty) && lineage.is_empty() {
                let mut failures = Vec::new();
                if let Some(error) = first_capture_error {
                    failures.push(format!(
                        "process snapshot failed during descendant cleanup: {error}"
                    ));
                }
                if !first_inaccessible.is_empty() {
                    failures.push(format!(
                        "descendant identity remained uncertain during cleanup; inaccessible={first_inaccessible:?}; details={first_inaccessible_details:?}"
                    ));
                }
                if failures.is_empty() {
                    return Ok(());
                }
                bail!(failures.join("; "));
            }
        }
    })();
    descendants.clear();
    lineage.clear();
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

fn process_command_line_from_handle(handle: HANDLE, storage: &mut [usize]) -> Result<OsString> {
    const BUFFER_BYTES: usize = 128 * 1024;
    let storage_bytes = size_of_val(storage);
    if storage_bytes < BUFFER_BYTES {
        bail!("process command line query storage is too small");
    }
    let mut returned = 0u32;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            storage.as_mut_ptr() as *mut c_void,
            storage_bytes as u32,
            &mut returned,
        )
    };
    if status < 0 {
        bail!(
            "failed to query process command line (ntstatus={:#010x})",
            status as u32
        );
    }
    if returned as usize > storage_bytes {
        bail!("process command line exceeded the query buffer");
    }
    let info = unsafe { &*(storage.as_ptr() as *const UNICODE_STRING) };
    if info.Length % 2 != 0 {
        bail!("process command line returned an odd UTF-16 byte length");
    }
    let buffer_start = storage.as_ptr() as usize;
    let buffer_end = buffer_start + storage_bytes;
    let text_start = info.Buffer as usize;
    let text_end = text_start
        .checked_add(info.Length as usize)
        .ok_or_else(|| anyhow::anyhow!("process command line pointer overflowed"))?;
    if info.Buffer.is_null() || text_start < buffer_start || text_end > buffer_end {
        bail!("process command line pointed outside the query buffer");
    }
    let text = unsafe { std::slice::from_raw_parts(info.Buffer, info.Length as usize / 2) };
    Ok(OsString::from_wide(text))
}

fn windows_command_line_arguments(command_line: &OsStr) -> Result<Vec<OsString>> {
    let mut command_line = wide_null(command_line);
    let mut argument_count = 0i32;
    let arguments = unsafe { CommandLineToArgvW(command_line.as_mut_ptr(), &mut argument_count) };
    if arguments.is_null() || argument_count <= 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to parse process command line");
    }
    let pointers = unsafe { std::slice::from_raw_parts(arguments, argument_count as usize) };
    let mut parsed = Vec::with_capacity(argument_count as usize);
    for pointer in pointers {
        let mut length = 0;
        unsafe {
            while *pointer.add(length) != 0 {
                length += 1;
            }
            parsed.push(OsString::from_wide(std::slice::from_raw_parts(
                *pointer, length,
            )));
        }
    }
    unsafe {
        LocalFree(arguments as *mut c_void);
    }
    Ok(parsed)
}

fn normalized_absolute_windows_path(value: &OsStr) -> Option<String> {
    let mut text = value.to_string_lossy().replace('/', "\\");
    if let Some(value) = text.strip_prefix(r"\\?\UNC\") {
        text = format!(r"\\{value}");
    } else if let Some(value) = text.strip_prefix(r"\\?\") {
        text = value.to_string();
    }
    let path = Path::new(&text);
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    Some(
        normalized
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase(),
    )
}

fn path_is_within_owned_root(value: &OsStr, root: &Path) -> bool {
    let Some(value) = normalized_absolute_windows_path(value) else {
        return false;
    };
    let Some(root) = normalized_absolute_windows_path(root.as_os_str()) else {
        return false;
    };
    if value == root {
        return true;
    }
    let prefix = if root.ends_with('\\') {
        root
    } else {
        format!(r"{root}\")
    };
    value.starts_with(&prefix)
}

fn option_value<'a>(argument: &'a str, option: &str) -> Option<&'a str> {
    let (name, value) = argument.split_once('=')?;
    (name == option).then_some(value)
}

fn resolve_git_working_directory(current: Option<&Path>, value: &OsStr) -> Option<PathBuf> {
    let value = Path::new(value);
    let candidate = if value.is_absolute() {
        value.to_path_buf()
    } else {
        current?.join(value)
    };
    normalized_absolute_windows_path(candidate.as_os_str()).map(PathBuf::from)
}

fn git_paths_reference_owned_root(
    working_directory: Option<&Path>,
    shallow_file: Option<&OsStr>,
    git_dir: Option<&OsStr>,
    work_tree: Option<&OsStr>,
    root: &Path,
) -> bool {
    working_directory.is_some_and(|path| path_is_within_owned_root(path.as_os_str(), root))
        || [shallow_file, git_dir, work_tree]
            .into_iter()
            .flatten()
            .filter_map(|path| resolve_git_working_directory(working_directory, path))
            .any(|path| path_is_within_owned_root(path.as_os_str(), root))
}

fn git_arguments_reference_owned_root(arguments: &[OsString], root: &Path) -> bool {
    let mut index = 1;
    let mut working_directory = None::<PathBuf>;
    let mut shallow_file = None::<OsString>;
    let mut git_dir = None::<OsString>;
    let mut work_tree = None::<OsString>;
    while index < arguments.len() {
        let argument = arguments[index].to_string_lossy();
        if argument == "--" || !argument.starts_with('-') || argument == "-" {
            return git_paths_reference_owned_root(
                working_directory.as_deref(),
                shallow_file.as_deref(),
                git_dir.as_deref(),
                work_tree.as_deref(),
                root,
            );
        }
        if argument == "-C" {
            let Some(value) = arguments.get(index + 1) else {
                return false;
            };
            working_directory = resolve_git_working_directory(working_directory.as_deref(), value);
            index += 2;
            continue;
        }
        if matches!(
            argument.as_ref(),
            "--shallow-file" | "--git-dir" | "--work-tree"
        ) {
            let Some(value) = arguments.get(index + 1).cloned() else {
                return false;
            };
            match argument.as_ref() {
                "--shallow-file" => shallow_file = Some(value),
                "--git-dir" => git_dir = Some(value),
                "--work-tree" => work_tree = Some(value),
                _ => unreachable!(),
            }
            index += 2;
            continue;
        }
        let mut stored_path_option = false;
        for option in ["--shallow-file", "--git-dir", "--work-tree"] {
            if let Some(value) = option_value(&argument, option) {
                let value = OsString::from(value);
                match option {
                    "--shallow-file" => shallow_file = Some(value),
                    "--git-dir" => git_dir = Some(value),
                    "--work-tree" => work_tree = Some(value),
                    _ => unreachable!(),
                }
                stored_path_option = true;
                break;
            }
        }
        if stored_path_option {
            index += 1;
            continue;
        }
        if matches!(
            argument.as_ref(),
            "-c" | "--config-env" | "--exec-path" | "--namespace" | "--super-prefix"
        ) {
            index += 2;
            continue;
        }
        if [
            "--config-env",
            "--exec-path",
            "--namespace",
            "--super-prefix",
        ]
        .iter()
        .any(|option| option_value(&argument, option).is_some())
            || matches!(
                argument.as_ref(),
                "-p" | "-P"
                    | "--paginate"
                    | "--no-pager"
                    | "--no-replace-objects"
                    | "--bare"
                    | "--literal-pathspecs"
                    | "--glob-pathspecs"
                    | "--noglob-pathspecs"
                    | "--icase-pathspecs"
                    | "--no-optional-locks"
                    | "--no-advice"
                    | "--version"
                    | "--help"
            )
        {
            index += 1;
            continue;
        }
        return false;
    }
    git_paths_reference_owned_root(
        working_directory.as_deref(),
        shallow_file.as_deref(),
        git_dir.as_deref(),
        work_tree.as_deref(),
        root,
    )
}

fn powershell_arguments_reference_owned_root(arguments: &[OsString], root: &Path) -> bool {
    let mut index = 1;
    while index < arguments.len() {
        let argument = arguments[index].to_string_lossy();
        if argument.eq_ignore_ascii_case("-File") {
            return arguments
                .get(index + 1)
                .is_some_and(|value| path_is_within_owned_root(value, root));
        }
        if argument == "--%"
            || [
                "-Command",
                "-CommandWithArgs",
                "-EncodedCommand",
                "-c",
                "-ec",
                "-e",
                "-enc",
            ]
            .iter()
            .any(|option| argument.eq_ignore_ascii_case(option))
        {
            return false;
        }
        if [
            "-ExecutionPolicy",
            "-InputFormat",
            "-OutputFormat",
            "-WindowStyle",
            "-WorkingDirectory",
            "-Version",
            "-ConfigurationName",
            "-CustomPipeName",
            "-SettingsFile",
        ]
        .iter()
        .any(|option| argument.eq_ignore_ascii_case(option))
        {
            index += 2;
            continue;
        }
        if [
            "-NoProfile",
            "-NoLogo",
            "-NonInteractive",
            "-Mta",
            "-Sta",
            "-NoExit",
            "-NoProfileLoadTime",
        ]
        .iter()
        .any(|option| argument.eq_ignore_ascii_case(option))
        {
            index += 1;
            continue;
        }
        return false;
    }
    false
}

fn chromium_arguments_reference_owned_root(arguments: &[OsString], root: &Path) -> bool {
    let mut index = 1;
    while index < arguments.len() {
        let argument = arguments[index].to_string_lossy();
        if argument == "--" {
            return false;
        }
        for option in [
            "--user-data-dir",
            "--disk-cache-dir",
            "--crash-dumps-dir",
            "--log-file",
        ] {
            if argument.eq_ignore_ascii_case(option) {
                if arguments
                    .get(index + 1)
                    .is_some_and(|value| path_is_within_owned_root(value, root))
                {
                    return true;
                }
                index += 1;
                break;
            }
            if let Some(value) = option_value(&argument.to_ascii_lowercase(), option)
                && path_is_within_owned_root(OsStr::new(value), root)
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn executable_name(value: &OsStr) -> Option<String> {
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
}

fn executable_may_reference_owned_root(executable: &OsStr) -> bool {
    executable_name(executable).is_some_and(|name| {
        matches!(
            name.as_str(),
            "git"
                | "git.exe"
                | "powershell"
                | "powershell.exe"
                | "pwsh"
                | "pwsh.exe"
                | "chatgpt.exe"
                | "chrome.exe"
                | "chromium.exe"
                | "msedge.exe"
                | "electron.exe"
        )
    })
}

const fn owned_root_query_access() -> u32 {
    PROCESS_QUERY_LIMITED_INFORMATION
}

fn query_process_exit_code_is_running(process: &TrackedProcess) -> Result<bool> {
    let mut exit_code = 0;
    if unsafe { GetExitCodeProcess(process.handle, &mut exit_code) } == 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to query process exit code from query-only handle");
    }
    Ok(exit_code == STILL_ACTIVE as u32)
}

fn arguments_reference_owned_root(arguments: &[OsString], executable: &OsStr, root: &Path) -> bool {
    let Some(executable) = executable_name(executable) else {
        return false;
    };
    match executable.as_str() {
        "git" | "git.exe" => git_arguments_reference_owned_root(arguments, root),
        "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => {
            powershell_arguments_reference_owned_root(arguments, root)
        }
        "chatgpt.exe" | "chrome.exe" | "chromium.exe" | "msedge.exe" | "electron.exe" => {
            chromium_arguments_reference_owned_root(arguments, root)
        }
        _ => false,
    }
}

fn command_line_references_owned_root_for_executable(
    command_line: &OsStr,
    executable: &OsStr,
    root: &Path,
) -> bool {
    windows_command_line_arguments(command_line)
        .is_ok_and(|arguments| arguments_reference_owned_root(&arguments, executable, root))
}

#[cfg(test)]
fn command_line_references_owned_root(command_line: &OsStr, root: &Path) -> bool {
    windows_command_line_arguments(command_line).is_ok_and(|arguments| {
        arguments.first().is_some_and(|executable| {
            arguments_reference_owned_root(&arguments, executable.as_os_str(), root)
        })
    })
}

fn capture_owned_root_references(
    entries: &[ProcessSnapshotEntry],
    root: &Path,
    descendants: &mut BTreeMap<ProcessIdentity, TrackedProcess>,
) -> Result<usize> {
    let current_pid = unsafe { GetCurrentProcessId() };
    let mut added = 0;
    let mut command_line_storage = vec![0usize; (128 * 1024usize).div_ceil(size_of::<usize>())];
    for entry in entries {
        if entry.pid == 0 || entry.pid == current_pid {
            continue;
        }
        let query_handle = unsafe { OpenProcess(owned_root_query_access(), 0, entry.pid) };
        if query_handle.is_null() {
            continue;
        }
        let query_process = match TrackedProcess::from_handle(entry.pid, query_handle) {
            Ok(process) => process,
            Err(_) => continue,
        };
        let image = process_image_path_from_handle(query_process.handle).ok();
        let command_line_matches = match image.as_deref() {
            Some(image) if path_is_within_owned_root(image.as_os_str(), root) => true,
            Some(image) if executable_may_reference_owned_root(image.as_os_str()) => {
                match process_command_line_from_handle(
                    query_process.handle,
                    &mut command_line_storage,
                ) {
                    Ok(command_line) => command_line_references_owned_root_for_executable(
                        &command_line,
                        image.as_os_str(),
                        root,
                    ),
                    Err(_) => false,
                }
            }
            _ => false,
        };
        if !command_line_matches {
            drop(query_process);
            continue;
        }
        let terminate_handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE_ACCESS,
                0,
                entry.pid,
            )
        };
        if terminate_handle.is_null() {
            let error = std::io::Error::last_os_error();
            if !query_process_exit_code_is_running(&query_process)? {
                drop(query_process);
                continue;
            }
            drop(query_process);
            return Err(error).with_context(|| {
                format!(
                    "failed to acquire termination rights for owned-root process {}",
                    entry.pid
                )
            });
        }
        let process = TrackedProcess::from_handle(entry.pid, terminate_handle)?;
        if process.identity != query_process.identity {
            let original_is_running = query_process_exit_code_is_running(&query_process)?;
            drop(process);
            drop(query_process);
            if !original_is_running {
                continue;
            }
            bail!("owned-root process identity changed before termination rights were acquired");
        }
        if !process_handle_is_running(process.handle)? {
            drop(process);
            drop(query_process);
            continue;
        }
        drop(query_process);
        if descendants.contains_key(&process.identity) {
            drop(process);
            continue;
        }
        if descendants
            .values()
            .filter(|existing| existing.identity.pid == entry.pid)
            .any(|existing| process_handle_is_running(existing.handle).unwrap_or(true))
        {
            drop(process);
            bail!("owned-root process reused the PID of a live tracked process");
        }
        descendants.insert(process.identity, process);
        added += 1;
    }
    Ok(added)
}

fn wait_for_condition_until<F>(deadline: Instant, poll_interval: Duration, mut complete: F) -> bool
where
    F: FnMut() -> bool,
{
    loop {
        if Instant::now() >= deadline {
            return false;
        }
        if complete() {
            return Instant::now() < deadline;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        thread::sleep(poll_interval.min(deadline - now));
    }
}

fn terminate_owned_root_references_once(root: &Path, deadline: Instant) -> Result<usize> {
    if Instant::now() >= deadline {
        bail!("owned-root process scan reached its deadline");
    }
    let snapshot = snapshot_process_entries()?;
    let mut tracked = BTreeMap::new();
    let captured = capture_owned_root_references(&snapshot.entries, root, &mut tracked)?;
    if Instant::now() >= deadline {
        tracked.clear();
        bail!("owned-root process scan completed after its deadline");
    }
    for process in tracked.values() {
        if process_handle_is_running(process.handle).unwrap_or(true) {
            unsafe {
                TerminateProcess(process.handle, 0);
            }
        }
    }
    let completed = wait_for_condition_until(deadline, Duration::from_millis(25), || {
        !tracked
            .values()
            .any(|process| process_handle_is_running(process.handle).unwrap_or(true))
    });
    let live = tracked
        .values()
        .filter(|process| process_handle_is_running(process.handle).unwrap_or(true))
        .map(|process| process.identity.pid)
        .collect::<BTreeSet<_>>();
    tracked.clear();
    if !completed || !live.is_empty() {
        bail!("owned-root processes did not terminate; live={live:?}");
    }
    Ok(captured)
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
    validate_snapshot_clock(captured_at, current_filetime())?;
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

fn lineage_process_ids(entries: &[(u32, u32)], roots: &BTreeSet<u32>) -> BTreeSet<u32> {
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

fn uncertain_lineage_process_ids(
    topology_entries: &[(u32, u32)],
    current_entries: &[(u32, u32)],
    roots: &BTreeSet<u32>,
    members: &BTreeSet<u32>,
) -> BTreeSet<u32> {
    let present = current_entries
        .iter()
        .map(|(pid, _)| *pid)
        .collect::<BTreeSet<_>>();
    let mut uncertain = lineage_process_ids(topology_entries, roots);
    uncertain
        .retain(|pid| present.contains(pid) && (!roots.contains(pid) || members.contains(pid)));
    uncertain
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
            lineage_process_ids(&entries, &BTreeSet::from([10])),
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
    fn process_generation_rejects_a_child_at_the_parent_exit_boundary() {
        let parent = ProcessGeneration {
            created_at: 100,
            exited_at: Some(300),
        };

        assert!(!parent.can_parent(300, 400));
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
    fn unresolved_child_of_a_known_parent_requires_a_temporary_anchor() {
        assert!(unresolved_descendant_requires_lineage_anchor(true));
        assert!(!unresolved_descendant_requires_lineage_anchor(false));
    }

    #[test]
    fn only_live_inaccessible_snapshot_processes_create_identity_uncertainty() {
        assert!(snapshot_process_requires_uncertainty(
            &SnapshotProcess::Inaccessible {
                error_code: Some(5)
            }
        ));
        assert!(!snapshot_process_requires_uncertainty(
            &SnapshotProcess::VanishedBeforeOpen
        ));
        assert!(!snapshot_process_requires_uncertainty(
            &SnapshotProcess::ReusedAfterSnapshot
        ));
    }

    #[test]
    fn vanished_and_reused_snapshot_processes_create_temporary_lineage_anchors() {
        assert!(snapshot_process_requires_lineage_anchor(
            &SnapshotProcess::VanishedBeforeOpen
        ));
        assert!(snapshot_process_requires_lineage_anchor(
            &SnapshotProcess::ReusedAfterSnapshot
        ));
        assert!(!snapshot_process_requires_lineage_anchor(
            &SnapshotProcess::Inaccessible {
                error_code: Some(5)
            }
        ));
    }

    #[test]
    fn snapshot_generation_equality_is_treated_as_pid_reuse() {
        assert!(!snapshot_generation_precedes_capture(100, 100));
        assert!(!snapshot_generation_precedes_capture(101, 100));
        assert!(snapshot_generation_precedes_capture(99, 100));
    }

    #[test]
    fn snapshot_clock_rollback_fails_closed() {
        assert!(validate_snapshot_clock(100, 99).is_err());
        assert!(validate_snapshot_clock(100, 100).is_ok());
        assert!(validate_snapshot_clock(100, 101).is_ok());
    }

    #[test]
    fn cross_snapshot_clock_rollback_fails_before_lineage_processing() {
        assert!(validate_snapshot_progress(Some(100), 99).is_err());
        assert!(validate_snapshot_progress(Some(100), 100).is_ok());
        assert!(validate_snapshot_progress(Some(100), 101).is_ok());
    }

    #[test]
    fn open_failure_requires_a_second_snapshot_before_permanent_uncertainty() {
        let vanished = classify_snapshot_open_failure(Some(5), false);
        assert!(matches!(vanished, SnapshotProcess::VanishedBeforeOpen));

        let inaccessible = classify_snapshot_open_failure(Some(5), true);
        assert!(matches!(
            inaccessible,
            SnapshotProcess::Inaccessible {
                error_code: Some(5)
            }
        ));
    }

    #[test]
    fn uncertain_lineage_includes_all_descendants_of_a_temporary_anchor() {
        let entries = [(20, 10), (30, 20), (40, 30), (50, 999)];

        assert_eq!(
            lineage_process_ids(&entries, &BTreeSet::from([20])),
            BTreeSet::from([20, 30, 40])
        );
    }

    #[test]
    fn promoted_lineage_member_keeps_a_grandchild_tainted_after_parent_exit() {
        let anchors = BTreeSet::from([20, 30, 40]);
        let members = BTreeSet::from([30, 40]);
        let entries = [(40, 30)];

        assert_eq!(
            uncertain_lineage_process_ids(&entries, &entries, &anchors, &members),
            BTreeSet::from([40])
        );
    }

    #[test]
    fn historical_lineage_edge_connects_a_surviving_grandchild() {
        let start = Instant::now();
        let mut history = ObservedLineageEdges::default();
        history.observe(&[(30, 20)], start);
        history.observe(&[(40, 30)], start + Duration::from_secs(1));

        assert_eq!(
            uncertain_lineage_process_ids(
                &history.pairs(),
                &[(40, 30)],
                &BTreeSet::from([20]),
                &BTreeSet::new()
            ),
            BTreeSet::from([40])
        );

        history.prune(start + Duration::from_secs(7), Duration::from_secs(5));
        assert!(history.pairs().is_empty());
    }

    #[test]
    fn historical_lineage_edge_does_not_make_a_missing_member_present() {
        assert_eq!(
            uncertain_lineage_process_ids(
                &[(30, 20)],
                &[],
                &BTreeSet::from([20]),
                &BTreeSet::from([30])
            ),
            BTreeSet::new()
        );
    }

    #[test]
    fn capture_input_prunes_expired_lineage_before_exporting_it() {
        let start = Instant::now();
        let mut lineage = LineageObservationState::default();
        lineage.anchors.insert(20, start);
        lineage.members.insert(20);
        lineage.edges.observe(&[(30, 20)], start);
        lineage.last_snapshot_at = Some(42);

        let input = lineage.capture_input(start + Duration::from_secs(6), Duration::from_secs(5));

        assert!(input.anchors.is_empty());
        assert!(input.members.is_empty());
        assert!(input.edges.is_empty());
        assert_eq!(input.previous_snapshot_at, Some(42));
    }

    #[test]
    fn command_line_root_match_is_case_and_separator_insensitive() {
        let root = Path::new(r"C:\Users\Example\Instance");

        assert!(command_line_references_owned_root(
            OsStr::new(r"git -C c:/users/example/instance/codex-home fetch"),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(
                r"git --shallow-file C:/Users/Example/Instance/repo/.git/shallow.lock index-pack"
            ),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(r"git -C \\?\C:\Users\Example\Instance\repo fetch"),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(r"git -C C:\Users\Example -C Instance status"),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(r"git -C C:\ -C Users\Example\Instance status"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"git -C C:\Users\Example\Instance -C .. status"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"git --git-dir=C:\Users\Example\Instance\repo --git-dir=C:\outside status"),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(r"git --git-dir=C:\outside --git-dir=C:\Users\Example\Instance\repo status"),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(r"git -C C:\Users\Example --git-dir=Instance\repo\.git status"),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(r"git -C C:\Users\Example --work-tree=Instance\repo status"),
            root
        ));
        assert!(command_line_references_owned_root(
            OsStr::new(
                r"git -C C:\Users\Example --shallow-file=Instance\repo\.git\shallow index-pack"
            ),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"git -C c:/users/example/instance-other fetch"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"prefixc:/users/example/instance/codex-home"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r#"powershell.exe -Command "Write-Output C:\Users\Example\Instance""#),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"tool.exe C:\Users\Example\Instance\..\unrelated"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"C:\Users\Example\Instance\pretend.exe --unrelated"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"tool.exe --shallow-file C:\Users\Example\Instance\repo\shallow"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"git -- -C C:\Users\Example\Instance fetch"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(r"git --message -C C:\Users\Example\Instance"),
            root
        ));
        assert!(!command_line_references_owned_root(
            OsStr::new(
                r#"powershell.exe -Command Write-Output -File C:\Users\Example\Instance\script.ps1"#
            ),
            root
        ));
    }

    #[test]
    fn command_line_parser_uses_the_real_image_instead_of_argv_zero() {
        let root = Path::new(r"C:\Users\Example\Instance");

        assert!(!command_line_references_owned_root_for_executable(
            OsStr::new(r"git.exe -C C:\Users\Example\Instance status"),
            OsStr::new("powershell.exe"),
            root
        ));
        assert!(command_line_references_owned_root_for_executable(
            OsStr::new(r"pretend.exe -C C:\Users\Example\Instance status"),
            OsStr::new("git.exe"),
            root
        ));
    }

    #[test]
    fn owned_root_scanner_limits_command_line_parsing_to_supported_brokers() {
        assert_eq!(owned_root_query_access(), PROCESS_QUERY_LIMITED_INFORMATION);
        assert_eq!(owned_root_query_access() & PROCESS_SYNCHRONIZE_ACCESS, 0);
        for executable in [
            "git.exe",
            "powershell.exe",
            "pwsh.exe",
            "ChatGPT.exe",
            "chrome.exe",
        ] {
            assert!(executable_may_reference_owned_root(OsStr::new(executable)));
        }
        assert!(!executable_may_reference_owned_root(OsStr::new(
            "unrelated.exe"
        )));
    }

    #[test]
    fn query_only_handle_reports_running_state_from_exit_code() {
        let mut child = Command::new(powershell_path())
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .spawn()
            .unwrap();
        let handle = unsafe { OpenProcess(owned_root_query_access(), 0, child.id()) };
        assert!(!handle.is_null());
        let process = TrackedProcess::from_handle(child.id(), handle).unwrap();

        assert!(query_process_exit_code_is_running(&process).unwrap());
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!query_process_exit_code_is_running(&process).unwrap());
    }

    #[test]
    fn owned_root_command_line_capture_tracks_an_external_process() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("instance-root");
        fs::create_dir(&root).unwrap();
        let script = root.join("sleep.ps1");
        fs::write(&script, "Start-Sleep -Seconds 30\n").unwrap();
        let mut child = Command::new(powershell_path())
            .args([
                OsStr::new("-NoProfile"),
                OsStr::new("-NonInteractive"),
                OsStr::new("-File"),
                script.as_os_str(),
            ])
            .spawn()
            .unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut tracked = BTreeMap::<ProcessIdentity, TrackedProcess>::new();
        while Instant::now() < deadline && !tracked.keys().any(|identity| identity.pid == pid) {
            let snapshot = snapshot_process_entries().unwrap();
            capture_owned_root_references(&snapshot.entries, &root, &mut tracked).unwrap();
            thread::sleep(Duration::from_millis(25));
        }

        let captured = tracked.keys().any(|identity| identity.pid == pid);
        for process in tracked.values() {
            unsafe {
                TerminateProcess(process.handle, 0);
            }
        }
        tracked.clear();
        let _ = child.kill();
        let _ = child.wait();

        assert!(captured);
    }

    #[test]
    fn drop_cleanup_timeout_exceeds_the_required_quiescence_window() {
        assert!(DROP_DESCENDANT_TIMEOUT > DESCENDANT_QUIESCENCE_DURATION);
    }

    #[test]
    fn descendant_quiescence_restarts_after_a_late_process() {
        let start = Instant::now();
        let mut quiescence = DescendantQuiescence::new(Duration::from_secs(5));

        assert!(!quiescence.observe(start, true));
        assert!(!quiescence.observe(start + Duration::from_secs(4), true));
        assert!(!quiescence.observe(start + Duration::from_millis(4_500), false));
        assert!(!quiescence.observe(start + Duration::from_secs(9), true));
        assert!(quiescence.observe(start + Duration::from_secs(14), true));
    }

    #[test]
    fn descendant_cleanup_does_not_succeed_after_its_deadline() {
        let mut descendants = BTreeMap::new();
        let mut lineage = LineageObservationState::default();

        let result = terminate_descendant_lineage_with_policy(
            &mut descendants,
            &mut lineage,
            Duration::from_millis(10),
            Duration::ZERO,
            |_, _, _, _, _| {
                thread::sleep(Duration::from_millis(50));
                Ok(CaptureOutcome::empty())
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[test]
    fn descendant_cleanup_deadline_includes_work_before_the_lineage_loop() {
        let mut descendants = BTreeMap::new();
        let mut lineage = LineageObservationState::default();
        let deadline = Instant::now() + Duration::from_millis(10);
        let mut captures = 0;
        thread::sleep(Duration::from_millis(30));

        let result = terminate_descendant_lineage_with_policy_until(
            &mut descendants,
            &mut lineage,
            deadline,
            Duration::ZERO,
            |_, _, _, _, _| {
                captures += 1;
                Ok(CaptureOutcome::empty())
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
        assert_eq!(captures, 0);
    }

    #[test]
    fn condition_wait_uses_the_full_remaining_deadline() {
        let started = Instant::now();
        let completed = wait_for_condition_until(
            started + Duration::from_millis(250),
            Duration::from_millis(10),
            || started.elapsed() >= Duration::from_millis(120),
        );

        assert!(completed);
        assert!(started.elapsed() >= Duration::from_millis(100));
    }

    #[test]
    fn condition_wait_rejects_completion_after_the_deadline() {
        let completed = wait_for_condition_until(
            Instant::now() + Duration::from_millis(10),
            Duration::ZERO,
            || {
                thread::sleep(Duration::from_millis(30));
                true
            },
        );

        assert!(!completed);
    }

    #[test]
    fn temporary_anchor_descendant_must_disappear_before_cleanup_succeeds() {
        let mut descendants = BTreeMap::new();
        let mut lineage = LineageObservationState::default();
        let mut captures = 0;
        let mut observed_anchor = false;

        let result = terminate_descendant_lineage_with_policy(
            &mut descendants,
            &mut lineage,
            Duration::from_secs(1),
            Duration::from_millis(100),
            |_, active_anchors, _, _, _| {
                captures += 1;
                if captures == 1 {
                    return Ok(CaptureOutcome {
                        lineage_anchors: BTreeSet::from([20]),
                        ..CaptureOutcome::empty()
                    });
                }
                if captures == 2 {
                    observed_anchor = active_anchors.contains(&20);
                    return Ok(CaptureOutcome {
                        uncertain_lineage: BTreeSet::from([30]),
                        ..CaptureOutcome::empty()
                    });
                }
                Ok(CaptureOutcome::empty())
            },
        );

        assert!(observed_anchor);
        assert!(result.is_ok());
        assert!(captures >= 4);
    }

    #[test]
    fn persistent_temporary_anchor_descendant_times_out_cleanup() {
        let mut descendants = BTreeMap::new();
        let mut lineage = LineageObservationState::default();
        let mut captures = 0;

        let result = terminate_descendant_lineage_with_policy(
            &mut descendants,
            &mut lineage,
            Duration::from_millis(220),
            Duration::from_millis(100),
            |_, _, _, _, _| {
                captures += 1;
                if captures == 1 {
                    return Ok(CaptureOutcome {
                        lineage_anchors: BTreeSet::from([20]),
                        ..CaptureOutcome::empty()
                    });
                }
                Ok(CaptureOutcome {
                    uncertain_lineage: BTreeSet::from([30]),
                    ..CaptureOutcome::empty()
                })
            },
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
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
                        ..CaptureOutcome::empty()
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
                    ..CaptureOutcome::empty()
                })
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn owned_job_terminates_a_spawned_process() {
        let executable = powershell_path();
        let temp = tempfile::tempdir().unwrap();
        let mut job = OwnedJob::create(temp.path().join("owned-root")).unwrap();
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
