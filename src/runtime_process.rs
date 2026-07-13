use std::{process::ExitStatus, process::Stdio};

use anyhow::{Context, Result, bail};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::mpsc,
};

use crate::{JsonlEvent, JsonlTransport, RuntimeLaunchSpec};

pub struct RuntimeProcess {
    _job: ProcessJob,
    child: Child,
    transport: JsonlTransport,
    events: mpsc::UnboundedReceiver<JsonlEvent>,
    stderr: mpsc::UnboundedReceiver<String>,
}

impl RuntimeProcess {
    pub async fn spawn(spec: RuntimeLaunchSpec, max_line_bytes: usize) -> Result<Self> {
        if spec.use_shell {
            bail!("runtime shell execution is forbidden");
        }
        RuntimeLaunchSpec::validate_executable_path(&spec.executable)?;

        let mut child = Command::new(&spec.executable)
            .args(&spec.args)
            .envs(&spec.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to launch {} runtime", runtime_name(&spec)))?;
        let job = match ProcessJob::attach(&child) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(error).context("failed to contain runtime process tree");
            }
        };
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("runtime stdin pipe was not created"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("runtime stdout pipe was not created"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("runtime stderr pipe was not created"))?;
        let (transport, events) = JsonlTransport::spawn(stdout, stdin, max_line_bytes);
        let stderr = spawn_stderr_reader(stderr, max_line_bytes.max(1));

        Ok(Self {
            _job: job,
            child,
            transport,
            events,
            stderr,
        })
    }

    pub fn transport(&self) -> JsonlTransport {
        self.transport.clone()
    }

    pub fn events_mut(&mut self) -> &mut mpsc::UnboundedReceiver<JsonlEvent> {
        &mut self.events
    }

    pub fn stderr_mut(&mut self) -> &mut mpsc::UnboundedReceiver<String> {
        &mut self.stderr
    }

    pub async fn wait(&mut self) -> Result<ExitStatus> {
        self.child
            .wait()
            .await
            .context("failed to wait for runtime")
    }

    pub async fn terminate(&mut self) -> Result<ExitStatus> {
        if self.child.try_wait()?.is_none() {
            self.child
                .kill()
                .await
                .context("failed to terminate runtime")?;
        }
        self.wait().await
    }
}

fn spawn_stderr_reader(
    stderr: tokio::process::ChildStderr,
    max_line_bytes: usize,
) -> mpsc::UnboundedReceiver<String> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            let read = match reader.read_until(b'\n', &mut buffer).await {
                Ok(read) => read,
                Err(error) => {
                    let _ = sender.send(format!("stderr read failed: {error}"));
                    return;
                }
            };
            if read == 0 {
                return;
            }
            if buffer.len() > max_line_bytes.saturating_add(1) {
                buffer.truncate(max_line_bytes);
            }
            while matches!(buffer.last(), Some(b'\n' | b'\r')) {
                buffer.pop();
            }
            let _ = sender.send(String::from_utf8_lossy(&buffer).into_owned());
        }
    });
    receiver
}

fn runtime_name(spec: &RuntimeLaunchSpec) -> &'static str {
    match spec.kind {
        crate::RuntimeKind::Codex => "Codex",
    }
}

#[cfg(windows)]
struct ProcessJob(usize);

#[cfg(windows)]
impl ProcessJob {
    fn attach(child: &Child) -> Result<Self> {
        use windows_sys::Win32::{
            Foundation::HANDLE,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
        };

        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error()).context("failed to create Windows job");
        }
        let job = Self(handle as usize);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(std::io::Error::last_os_error()).context("failed to configure Windows job");
        }
        let process = child
            .raw_handle()
            .ok_or_else(|| anyhow::anyhow!("runtime exited before job assignment"))?
            as HANDLE;
        let assigned = unsafe { AssignProcessToJobObject(handle, process) };
        if assigned == 0 {
            return Err(std::io::Error::last_os_error())
                .context("failed to assign runtime to Windows job");
        }
        Ok(job)
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(
                self.0 as windows_sys::Win32::Foundation::HANDLE,
            );
        }
    }
}

#[cfg(not(windows))]
struct ProcessJob;

#[cfg(not(windows))]
impl ProcessJob {
    fn attach(_child: &Child) -> Result<Self> {
        Ok(Self)
    }
}
