#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("Codex Administrator Launcher currently supports Windows only.");
}

#[cfg(windows)]
mod windows_launcher {
    use std::{
        env, fs,
        io::Read,
        path::PathBuf,
        process::{ChildStderr, ChildStdout},
        thread,
    };

    use anyhow::{Context, Result, bail};
    use codex_administrator::{
        CredentialStore, LauncherChildEvent, LauncherChildOutcome, LauncherSupervisorBackend,
        PROVIDER_CREDENTIAL_TARGET, SupervisorGeneration, WindowsCredentialStore,
        launcher_instance_root, launcher_settings_path, load_launcher_settings,
        parse_launcher_child_event, resolve_bound_provider_credential,
        sanitize_launcher_diagnostic, spawn_direct_launcher, supervise_launcher,
    };
    use zeroize::Zeroizing;

    const MAX_MONITORED_STREAM_BYTES: usize = 64 * 1024;
    const MAX_RESTARTS: usize = 32;

    pub fn run() -> Result<()> {
        let mut backend = ProcessSupervisor::new()?;
        supervise_launcher(&mut backend, MAX_RESTARTS)?;
        Ok(())
    }

    pub fn record_fatal_error(error: &anyhow::Error) {
        let Ok(settings_path) = launcher_settings_path() else {
            return;
        };
        let Some(root) = settings_path.parent() else {
            return;
        };
        if fs::create_dir_all(root).is_err() {
            return;
        }
        let message = format!("Codex Administrator launcher failed:\n{error:#}\n");
        let _ = fs::write(root.join("launcher-error.log"), message);
    }

    struct ProcessSupervisor {
        settings_path: PathBuf,
        instance_root: PathBuf,
        child_executable: PathBuf,
        credential_store: WindowsCredentialStore,
    }

    impl ProcessSupervisor {
        fn new() -> Result<Self> {
            Ok(Self {
                settings_path: launcher_settings_path()?,
                instance_root: launcher_instance_root()?,
                child_executable: sibling_cli_executable()?,
                credential_store: WindowsCredentialStore::new(PROVIDER_CREDENTIAL_TARGET),
            })
        }
    }

    impl LauncherSupervisorBackend for ProcessSupervisor {
        fn load_generation(&mut self) -> Result<SupervisorGeneration> {
            let settings = load_launcher_settings(&self.settings_path)?;
            let stored = self.credential_store.read()?.map(Zeroizing::new);
            let credential = match stored.as_deref() {
                Some(stored) => resolve_bound_provider_credential(
                    &settings.base_url,
                    &settings.action_path,
                    stored,
                )?,
                None => None,
            };
            SupervisorGeneration::new(settings, credential)
        }

        fn run_generation(
            &mut self,
            generation: &SupervisorGeneration,
        ) -> Result<LauncherChildOutcome> {
            let mut child = spawn_direct_launcher(
                &self.child_executable,
                generation.settings(),
                &self.instance_root,
                generation.credential(),
                generation.credential_present(),
            )?;
            let Some(stdout) = child.stdout.take() else {
                terminate_child(&mut child);
                bail!("isolated child did not expose its status stream");
            };
            let Some(stderr) = child.stderr.take() else {
                terminate_child(&mut child);
                bail!("isolated child did not expose its diagnostic stream");
            };
            let stderr_reader = thread::spawn(move || read_bounded_tail(stderr));
            let events = monitor_child_stdout(stdout);
            let status = child
                .wait()
                .context("failed to wait for the isolated child")?;
            let stderr = stderr_reader.join().unwrap_or_default();
            let diagnostic =
                sanitize_launcher_diagnostic(&stderr, generation.credential().unwrap_or_default());
            let events = events?;
            Ok(LauncherChildOutcome {
                ready_mode: events.ready_mode,
                restart_requested: events.restart_requested,
                success: status.success(),
                exit_code: status.code(),
                diagnostic,
            })
        }
    }

    #[derive(Default)]
    struct ObservedEvents {
        ready_mode: Option<codex_administrator::SupervisorMode>,
        restart_requested: bool,
    }

    fn monitor_child_stdout(mut stdout: ChildStdout) -> Result<ObservedEvents> {
        let mut observed = ObservedEvents::default();
        let mut chunk = [0_u8; 4096];
        let mut line = Vec::new();
        let mut overflowed = false;
        loop {
            let read = stdout
                .read(&mut chunk)
                .context("failed to read the isolated child status stream")?;
            if read == 0 {
                break;
            }
            for byte in &chunk[..read] {
                if *byte == b'\n' {
                    if !overflowed {
                        observe_line(&line, &mut observed)?;
                    }
                    line.clear();
                    overflowed = false;
                } else if line.len() < MAX_MONITORED_STREAM_BYTES {
                    line.push(*byte);
                } else {
                    overflowed = true;
                }
            }
        }
        if !line.is_empty() && !overflowed {
            observe_line(&line, &mut observed)?;
        }
        Ok(observed)
    }

    fn observe_line(line: &[u8], observed: &mut ObservedEvents) -> Result<()> {
        let line = String::from_utf8_lossy(line);
        let Some(event) = parse_launcher_child_event(line.trim()) else {
            return Ok(());
        };
        match event {
            LauncherChildEvent::Ready { mode } => {
                if observed.ready_mode.is_some_and(|current| current != mode) {
                    bail!("isolated child reported conflicting readiness modes");
                }
                observed.ready_mode = Some(mode);
            }
            LauncherChildEvent::RestartRequested => {
                if observed.ready_mode.is_none() {
                    bail!("isolated child requested restart before readiness");
                }
                observed.restart_requested = true;
            }
        }
        Ok(())
    }

    fn read_bounded_tail(mut stderr: ChildStderr) -> Vec<u8> {
        let mut output = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let read = match stderr.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => read,
                Err(_) => break,
            };
            if read >= MAX_MONITORED_STREAM_BYTES {
                output.clear();
                output.extend_from_slice(&chunk[read - MAX_MONITORED_STREAM_BYTES..read]);
                continue;
            }
            let overflow = output
                .len()
                .saturating_add(read)
                .saturating_sub(MAX_MONITORED_STREAM_BYTES);
            if overflow > 0 {
                output.drain(..overflow);
            }
            output.extend_from_slice(&chunk[..read]);
        }
        output
    }

    fn terminate_child(child: &mut std::process::Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    fn sibling_cli_executable() -> Result<PathBuf> {
        let current = env::current_exe().context("failed to resolve launcher executable")?;
        let cli = current.with_file_name("codex-administrator.exe");
        if !cli.is_file() {
            bail!("codex-administrator.exe was not found beside the launcher");
        }
        Ok(cli)
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_launcher::run() {
        windows_launcher::record_fatal_error(&error);
        std::process::exit(1);
    }
}
