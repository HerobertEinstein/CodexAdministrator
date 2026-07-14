use std::{
    collections::BTreeSet,
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use http::Uri;
use serde::Serialize;

use crate::{DirectIsolationContract, IsolatedRuntimeObservation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectInstanceLayout {
    root: PathBuf,
    contract: DirectIsolationContract,
}

impl DirectInstanceLayout {
    pub fn new(
        root: PathBuf,
        executable: PathBuf,
        daily_profile: PathBuf,
        daily_codex_home: PathBuf,
        cdp_port: u16,
    ) -> Result<Self> {
        verify_project_owned_root_shape(&root)?;
        let contract = DirectIsolationContract::new(
            executable,
            daily_profile,
            root.join("profile"),
            daily_codex_home,
            root.join("codex-home"),
            cdp_port,
        )?;
        contract.verify_owned_root(&root)?;
        Ok(Self { root, contract })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn contract(&self) -> &DirectIsolationContract {
        &self.contract
    }

    pub fn verify_contract(&self, contract: &DirectIsolationContract) -> Result<()> {
        if contract != &self.contract {
            bail!("direct runtime contract does not match the owned instance layout");
        }
        contract.verify_owned_root(&self.root)
    }
}

fn verify_project_owned_root_shape(root: &Path) -> Result<()> {
    let session = root
        .file_name()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("isolated instance root requires a session directory"))?;
    if session.to_string_lossy().contains(['/', '\\']) {
        bail!("isolated instance session directory is invalid");
    }
    let instances = root
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    let product = root
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|value| value.to_str());
    if !instances.is_some_and(|value| value.eq_ignore_ascii_case("instances"))
        || !product.is_some_and(|value| value.eq_ignore_ascii_case("CodexAdministrator"))
    {
        bail!("isolated instance root must be under CodexAdministrator/instances");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectCdpTarget {
    pub id: String,
    pub page_url: String,
    pub websocket_url: String,
}

impl DirectCdpTarget {
    pub fn validate_for_port(&self, expected_port: u16) -> Result<()> {
        if self.id.is_empty() || self.page_url != "app://-/index.html" {
            bail!("direct CDP target is not the official app renderer");
        }
        let uri: Uri = self
            .websocket_url
            .parse()
            .map_err(|error| anyhow::anyhow!("direct CDP websocket URL is invalid: {error}"))?;
        if uri.scheme_str() != Some("ws")
            || !matches!(uri.host(), Some("127.0.0.1" | "localhost" | "::1"))
            || uri.port_u16() != Some(expected_port)
            || uri.query().is_some()
        {
            bail!("direct CDP websocket does not belong to the isolated loopback port");
        }
        let target_id = uri
            .path()
            .strip_prefix("/devtools/page/")
            .filter(|value| !value.is_empty());
        if target_id != Some(self.id.as_str()) {
            bail!("direct CDP websocket target id does not match the selected renderer");
        }
        Ok(())
    }
}

pub trait DirectRuntimeBackend {
    fn snapshot_chatgpt_pids(&mut self) -> Result<BTreeSet<u32>>;

    fn prepare_owned_paths(&mut self, contract: &DirectIsolationContract) -> Result<()>;

    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
    ) -> Result<()>;

    fn wait_for_cdp_endpoint(&mut self, port: u16, timeout: Duration) -> Result<()>;

    fn wait_for_app_target(&mut self, port: u16, timeout: Duration) -> Result<DirectCdpTarget>;

    fn owned_pids(&mut self) -> Result<BTreeSet<u32>>;

    fn cdp_listener_pid(&mut self, port: u16) -> Result<u32>;

    fn install_bootstrap(
        &mut self,
        target: &DirectCdpTarget,
        script: &str,
        timeout: Duration,
    ) -> Result<()>;

    fn wait_for_ui_ready(&mut self, target: &DirectCdpTarget, timeout: Duration) -> Result<()>;

    fn wait_for_provider_ready(
        &mut self,
        target: &DirectCdpTarget,
        timeout: Duration,
    ) -> Result<()>;

    fn injection_healthy(&mut self, target: &DirectCdpTarget) -> Result<bool>;

    fn shutdown(&mut self) -> Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectMaintenance {
    Healthy,
    Reinjected,
    Recovering,
}

pub struct DirectInstance<B: DirectRuntimeBackend> {
    contract: DirectIsolationContract,
    bootstrap: String,
    backend: Option<B>,
    preexisting_pids: BTreeSet<u32>,
    target: DirectCdpTarget,
    timeout: Duration,
    maintenance_failure_since: Option<Instant>,
}

impl<B: DirectRuntimeBackend> fmt::Debug for DirectInstance<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DirectInstance")
            .field("contract", &self.contract)
            .field("preexisting_pids", &self.preexisting_pids)
            .field("target", &self.target)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl<B: DirectRuntimeBackend> DirectInstance<B> {
    pub fn start(
        contract: DirectIsolationContract,
        bootstrap: String,
        mut backend: B,
        timeout: Duration,
    ) -> Result<Self> {
        let startup = (|| -> Result<(BTreeSet<u32>, DirectCdpTarget)> {
            let preexisting_pids = backend.snapshot_chatgpt_pids()?;
            backend.prepare_owned_paths(&contract)?;
            backend.launch(
                contract.executable(),
                &contract.initial_launch_arguments(),
                &contract.environment_overrides(),
            )?;
            backend.wait_for_cdp_endpoint(contract.cdp_port(), timeout)?;
            backend.launch(
                contract.executable(),
                &contract.activation_arguments(),
                &contract.environment_overrides(),
            )?;
            let target = backend.wait_for_app_target(contract.cdp_port(), timeout)?;
            target.validate_for_port(contract.cdp_port())?;
            verify_runtime(&contract, &preexisting_pids, &mut backend, &target)?;
            backend.install_bootstrap(&target, &bootstrap, timeout)?;
            backend.wait_for_ui_ready(&target, timeout)?;
            backend.wait_for_provider_ready(&target, timeout)?;
            Ok((preexisting_pids, target))
        })();

        match startup {
            Ok((preexisting_pids, target)) => Ok(Self {
                contract,
                bootstrap,
                backend: Some(backend),
                preexisting_pids,
                target,
                timeout,
                maintenance_failure_since: None,
            }),
            Err(error) => {
                if let Err(cleanup) = backend.shutdown() {
                    bail!("{error}; isolated runtime cleanup also failed: {cleanup}");
                }
                Err(error)
            }
        }
    }

    pub fn target(&self) -> &DirectCdpTarget {
        &self.target
    }

    pub fn maintain_once(&mut self) -> Result<DirectMaintenance> {
        let result = (|| -> Result<DirectMaintenance> {
            let backend = self
                .backend
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("direct instance is already shut down"))?;
            let target = match backend.wait_for_app_target(self.contract.cdp_port(), Duration::ZERO)
            {
                Ok(target) => target,
                Err(error) => {
                    return tolerate_transient_maintenance_failure(
                        &mut self.maintenance_failure_since,
                        self.timeout,
                        error,
                    );
                }
            };
            target.validate_for_port(self.contract.cdp_port())?;
            verify_runtime(&self.contract, &self.preexisting_pids, backend, &target)?;

            let target_changed = target != self.target;
            let healthy = if target_changed {
                false
            } else {
                match backend.injection_healthy(&target) {
                    Ok(healthy) => healthy,
                    Err(error) => {
                        return tolerate_transient_maintenance_failure(
                            &mut self.maintenance_failure_since,
                            self.timeout,
                            error,
                        );
                    }
                }
            };
            if !healthy {
                if let Err(error) =
                    backend.install_bootstrap(&target, &self.bootstrap, self.timeout)
                {
                    return tolerate_transient_maintenance_failure(
                        &mut self.maintenance_failure_since,
                        self.timeout,
                        error,
                    );
                }
                self.target = target;
                self.maintenance_failure_since = None;
                return Ok(DirectMaintenance::Reinjected);
            }
            self.maintenance_failure_since = None;
            Ok(DirectMaintenance::Healthy)
        })();

        if result.is_err() {
            let _ = self.shutdown_inner();
        }
        result
    }

    pub fn shutdown(&mut self) -> Result<()> {
        self.shutdown_inner()
    }

    fn shutdown_inner(&mut self) -> Result<()> {
        match self.backend.take() {
            Some(mut backend) => backend.shutdown(),
            None => Ok(()),
        }
    }
}

fn tolerate_transient_maintenance_failure(
    failed_since: &mut Option<Instant>,
    timeout: Duration,
    error: anyhow::Error,
) -> Result<DirectMaintenance> {
    let failed_since = failed_since.get_or_insert_with(Instant::now);
    if failed_since.elapsed() < timeout {
        Ok(DirectMaintenance::Recovering)
    } else {
        Err(error)
    }
}

impl<B: DirectRuntimeBackend> Drop for DirectInstance<B> {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

fn verify_runtime<B: DirectRuntimeBackend>(
    contract: &DirectIsolationContract,
    preexisting_pids: &BTreeSet<u32>,
    backend: &mut B,
    target: &DirectCdpTarget,
) -> Result<()> {
    let owned_pids = backend.owned_pids()?;
    let cdp_listener_pid = backend.cdp_listener_pid(contract.cdp_port())?;
    let current_chatgpt_pids = backend.snapshot_chatgpt_pids()?;
    contract.verify_runtime(&IsolatedRuntimeObservation {
        preexisting_pids: preexisting_pids.clone(),
        owned_pids,
        cdp_listener_pid,
        daily_root_alive: preexisting_pids.is_subset(&current_chatgpt_pids),
        cdp_port: contract.cdp_port(),
        cdp_target_url: Some(target.page_url.clone()),
    })
}
