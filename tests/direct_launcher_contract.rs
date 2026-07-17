use std::{
    collections::{BTreeSet, VecDeque},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Result, bail};
use codex_administrator::{
    ControlResponse, DirectCdpTarget, DirectInstance, DirectIsolationContract, DirectMaintenance,
    DirectRuntimeBackend,
};

fn contract() -> DirectIsolationContract {
    DirectIsolationContract::new(
        PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe"),
        PathBuf::from(r"C:\Users\Example\AppData\Roaming\Codex\web\Codex"),
        PathBuf::from(r"C:\Users\Example\AppData\Local\CodexAdministrator\instance\profile"),
        PathBuf::from(r"C:\Users\Example\.codex"),
        PathBuf::from(r"C:\Users\Example\AppData\Local\CodexAdministrator\instance\codex-home"),
        9341,
    )
    .unwrap()
}

fn app_target(id: &str) -> DirectCdpTarget {
    DirectCdpTarget {
        id: id.into(),
        page_url: "app://-/index.html".into(),
        websocket_url: format!("ws://127.0.0.1:9341/devtools/page/{id}"),
    }
}

#[derive(Clone)]
struct FakeRuntime {
    state: Arc<Mutex<FakeState>>,
}

struct FakeState {
    events: Vec<String>,
    snapshots: VecDeque<BTreeSet<u32>>,
    fallback_snapshot: BTreeSet<u32>,
    owned_pids: BTreeSet<u32>,
    cdp_listener_pid: u32,
    targets: VecDeque<DirectCdpTarget>,
    current_target: Option<DirectCdpTarget>,
    healthy: bool,
    fail_health: bool,
    fail_control_drain: bool,
    fail_control_delivery: bool,
    fail_target_wait: bool,
    fail_provider_ready: bool,
    provider_readiness_required: bool,
    window_open: bool,
    shutdowns: usize,
}

impl FakeRuntime {
    fn new(preexisting: BTreeSet<u32>, current: BTreeSet<u32>, owned: BTreeSet<u32>) -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState {
                events: Vec::new(),
                snapshots: VecDeque::from([preexisting, current.clone()]),
                fallback_snapshot: current.clone(),
                cdp_listener_pid: owned.iter().next().copied().unwrap_or_default(),
                owned_pids: owned,
                targets: VecDeque::from([app_target("initial")]),
                current_target: None,
                healthy: true,
                fail_health: false,
                fail_control_drain: false,
                fail_control_delivery: false,
                fail_target_wait: false,
                fail_provider_ready: false,
                provider_readiness_required: true,
                window_open: true,
                shutdowns: 0,
            })),
        }
    }

    fn events(&self) -> Vec<String> {
        self.state.lock().unwrap().events.clone()
    }

    fn shutdowns(&self) -> usize {
        self.state.lock().unwrap().shutdowns
    }

    fn with_cdp_listener_pid(self, pid: u32) -> Self {
        self.state.lock().unwrap().cdp_listener_pid = pid;
        self
    }

    fn without_provider(self) -> Self {
        self.state.lock().unwrap().provider_readiness_required = false;
        self
    }
}

impl DirectRuntimeBackend for FakeRuntime {
    fn requires_provider_readiness(&self) -> bool {
        self.state.lock().unwrap().provider_readiness_required
    }

    fn snapshot_chatgpt_pids(&mut self) -> Result<BTreeSet<u32>> {
        let mut state = self.state.lock().unwrap();
        state.events.push("snapshot".into());
        Ok(state
            .snapshots
            .pop_front()
            .unwrap_or_else(|| state.fallback_snapshot.clone()))
    }

    fn prepare_owned_paths(&mut self, _contract: &DirectIsolationContract) -> Result<()> {
        self.state.lock().unwrap().events.push("prepare".into());
        Ok(())
    }

    fn launch(
        &mut self,
        _executable: &Path,
        arguments: &[OsString],
        environment: &[(OsString, OsString)],
    ) -> Result<()> {
        assert_eq!(
            environment,
            [
                (
                    OsString::from("CODEX_HOME"),
                    OsString::from(
                        r"C:\Users\Example\AppData\Local\CodexAdministrator\instance\codex-home"
                    ),
                ),
                (
                    OsString::from("CODEX_SQLITE_HOME"),
                    OsString::from(
                        r"C:\Users\Example\AppData\Local\CodexAdministrator\instance\codex-home\sqlite"
                    ),
                ),
            ]
        );
        let stage = if arguments.last() == Some(&OsString::from("--new-window")) {
            "activation"
        } else {
            "initial"
        };
        self.state
            .lock()
            .unwrap()
            .events
            .push(format!("launch:{stage}"));
        Ok(())
    }

    fn wait_for_cdp_endpoint(&mut self, port: u16, _timeout: Duration) -> Result<()> {
        assert_eq!(port, 9341);
        self.state.lock().unwrap().events.push("cdp".into());
        Ok(())
    }

    fn wait_for_app_target(&mut self, port: u16, _timeout: Duration) -> Result<DirectCdpTarget> {
        assert_eq!(port, 9341);
        let mut state = self.state.lock().unwrap();
        state.events.push("target".into());
        if state.fail_target_wait {
            bail!("target unavailable");
        }
        let target = state
            .targets
            .pop_front()
            .or_else(|| state.current_target.clone())
            .ok_or_else(|| anyhow::anyhow!("no target"))?;
        state.current_target = Some(target.clone());
        Ok(target)
    }

    fn owned_pids(&mut self) -> Result<BTreeSet<u32>> {
        let mut state = self.state.lock().unwrap();
        state.events.push("owned".into());
        Ok(state.owned_pids.clone())
    }

    fn cdp_listener_pid(&mut self, port: u16) -> Result<u32> {
        assert_eq!(port, 9341);
        let mut state = self.state.lock().unwrap();
        state.events.push("listener".into());
        Ok(state.cdp_listener_pid)
    }

    fn owned_window_open(&mut self) -> Result<bool> {
        Ok(self.state.lock().unwrap().window_open)
    }

    fn install_bootstrap(
        &mut self,
        target: &DirectCdpTarget,
        script: &str,
        _timeout: Duration,
    ) -> Result<()> {
        assert_eq!(target.page_url, "app://-/index.html");
        assert_eq!(script, "bootstrap();");
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("inject:{}", target.id));
        state.healthy = true;
        Ok(())
    }

    fn wait_for_ui_ready(&mut self, target: &DirectCdpTarget, _timeout: Duration) -> Result<()> {
        self.state
            .lock()
            .unwrap()
            .events
            .push(format!("ui:{}", target.id));
        Ok(())
    }

    fn wait_for_provider_ready(
        &mut self,
        target: &DirectCdpTarget,
        _timeout: Duration,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("provider:{}", target.id));
        if state.fail_provider_ready {
            bail!("model provider 'grok_native' not found");
        }
        Ok(())
    }

    fn injection_healthy(&mut self, target: &DirectCdpTarget) -> Result<bool> {
        let mut state = self.state.lock().unwrap();
        state.events.push(format!("health:{}", target.id));
        if state.fail_health {
            bail!("renderer health connection unavailable");
        }
        Ok(state.healthy)
    }

    fn drain_control_requests(
        &mut self,
        _target: &DirectCdpTarget,
        _nonce: &str,
    ) -> Result<Vec<codex_administrator::ControlRequest>> {
        let mut state = self.state.lock().unwrap();
        state.events.push("control:drain".into());
        if state.fail_control_drain {
            bail!("renderer control drain connection unavailable");
        }
        Ok(Vec::new())
    }

    fn deliver_control_response(
        &mut self,
        _target: &DirectCdpTarget,
        _response: ControlResponse,
    ) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.events.push("control:deliver".into());
        if state.fail_control_delivery {
            bail!("renderer control delivery connection unavailable");
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<()> {
        let mut state = self.state.lock().unwrap();
        state.events.push("shutdown".into());
        state.shutdowns += 1;
        Ok(())
    }
}

#[test]
fn starts_the_owned_instance_in_two_stages_before_installing_the_bootstrap() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100, 101]),
        BTreeSet::from([100, 101, 400, 401]),
        BTreeSet::from([400, 401]),
    );
    let observer = runtime.clone();

    let instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();

    assert_eq!(instance.target(), &app_target("initial"));
    assert_eq!(
        observer.events(),
        [
            "snapshot",
            "prepare",
            "launch:initial",
            "cdp",
            "launch:activation",
            "target",
            "owned",
            "listener",
            "snapshot",
            "inject:initial",
            "ui:initial",
            "provider:initial",
        ]
    );
    drop(instance);
    assert_eq!(observer.shutdowns(), 1);
}

#[test]
fn a_foreign_cdp_listener_fails_closed_before_injection() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100, 101]),
        BTreeSet::from([100, 101, 400]),
        BTreeSet::from([400]),
    )
    .with_cdp_listener_pid(999);
    let observer = runtime.clone();

    let error = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap_err();

    assert!(error.to_string().contains("CDP listener is not owned"));
    assert!(
        !observer
            .events()
            .iter()
            .any(|event| event.starts_with("inject:"))
    );
    assert_eq!(observer.shutdowns(), 1);
}

#[test]
fn overlap_with_a_preexisting_chatgpt_pid_fails_closed_before_injection() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100, 101]),
        BTreeSet::from([100, 101, 400]),
        BTreeSet::from([100, 400]),
    );
    let observer = runtime.clone();

    let error = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap_err();

    assert!(error.to_string().contains("pre-existing ChatGPT process"));
    assert!(
        !observer
            .events()
            .iter()
            .any(|event| event.starts_with("inject:"))
    );
    assert_eq!(observer.shutdowns(), 1);
}

#[test]
fn a_replaced_app_target_is_reinjected_without_touching_an_existing_target() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    runtime
        .state
        .lock()
        .unwrap()
        .targets
        .push_back(app_target("replacement"));
    let observer = runtime.clone();
    let mut instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();

    assert_eq!(
        instance.maintain_once().unwrap(),
        DirectMaintenance::Reinjected
    );
    assert_eq!(instance.target(), &app_target("replacement"));
    assert!(observer.events().contains(&"inject:replacement".into()));
}

#[test]
fn a_transient_missing_reload_target_does_not_close_the_owned_instance() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    let observer = runtime.clone();
    let mut instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();
    observer.state.lock().unwrap().fail_target_wait = true;

    assert!(instance.maintain_once().is_ok());
    assert_eq!(observer.shutdowns(), 0);

    let mut state = observer.state.lock().unwrap();
    state.fail_target_wait = false;
    state.targets.push_back(app_target("replacement"));
    drop(state);

    assert_eq!(
        instance.maintain_once().unwrap(),
        DirectMaintenance::Reinjected
    );
    assert_eq!(observer.shutdowns(), 0);
}

#[test]
fn closing_the_owned_window_is_a_clean_exit_instead_of_a_reload_failure() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    let observer = runtime.clone();
    let mut instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();
    {
        let mut state = observer.state.lock().unwrap();
        state.fail_target_wait = true;
        state.window_open = false;
    }

    assert_eq!(instance.maintain_once().unwrap(), DirectMaintenance::Exited);
    assert_eq!(observer.shutdowns(), 0);
    instance.shutdown().unwrap();
    assert_eq!(observer.shutdowns(), 1);
}

#[test]
fn a_transient_reload_health_disconnect_does_not_close_the_owned_instance() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    let observer = runtime.clone();
    let mut instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();
    observer.state.lock().unwrap().fail_health = true;

    assert!(instance.maintain_once().is_ok());
    assert_eq!(observer.shutdowns(), 0);

    let mut state = observer.state.lock().unwrap();
    state.fail_health = false;
    state.healthy = false;
    drop(state);

    assert_eq!(
        instance.maintain_once().unwrap(),
        DirectMaintenance::Reinjected
    );
    assert_eq!(observer.shutdowns(), 0);
}

#[test]
fn transient_control_transport_disconnects_do_not_close_the_owned_instance() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    let observer = runtime.clone();
    let mut instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();
    {
        let mut state = observer.state.lock().unwrap();
        state.fail_control_drain = true;
        state.fail_control_delivery = true;
    }

    assert!(instance.drain_control_requests("nonce").unwrap().is_empty());
    instance
        .deliver_control_response(ControlResponse::success(
            "request",
            "nonce",
            serde_json::json!({}),
        ))
        .unwrap();
    assert_eq!(observer.shutdowns(), 0);

    {
        let mut state = observer.state.lock().unwrap();
        state.fail_control_drain = false;
        state.fail_control_delivery = false;
    }
    assert_eq!(
        instance.maintain_once().unwrap(),
        DirectMaintenance::Healthy
    );
    assert_eq!(observer.shutdowns(), 0);
}

#[test]
fn an_unhealthy_current_target_is_reinjected_and_shutdown_is_idempotent() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    let observer = runtime.clone();
    let mut instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();
    observer.state.lock().unwrap().healthy = false;

    assert_eq!(
        instance.maintain_once().unwrap(),
        DirectMaintenance::Reinjected
    );
    instance.shutdown().unwrap();
    assert_eq!(observer.shutdowns(), 1);
}

#[test]
fn a_missing_target_during_startup_cleans_the_owned_runtime() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    runtime.state.lock().unwrap().fail_target_wait = true;
    let observer = runtime.clone();

    assert!(
        DirectInstance::start(
            contract(),
            "bootstrap();".into(),
            runtime,
            Duration::from_secs(1),
        )
        .is_err()
    );
    assert_eq!(observer.shutdowns(), 1);
    assert!(
        !observer
            .events()
            .iter()
            .any(|event| event.starts_with("inject:"))
    );
}

#[test]
fn an_unresolved_native_provider_fails_closed_before_ready() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    );
    runtime.state.lock().unwrap().fail_provider_ready = true;
    let observer = runtime.clone();

    let error = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap_err();

    assert!(error.to_string().contains("grok_native"));
    assert!(observer.events().contains(&"provider:initial".into()));
    assert_eq!(observer.shutdowns(), 1);
}

#[test]
fn management_only_startup_skips_native_provider_readiness() {
    let runtime = FakeRuntime::new(
        BTreeSet::from([100]),
        BTreeSet::from([100, 400]),
        BTreeSet::from([400]),
    )
    .without_provider();
    runtime.state.lock().unwrap().fail_provider_ready = true;
    let observer = runtime.clone();

    let instance = DirectInstance::start(
        contract(),
        "bootstrap();".into(),
        runtime,
        Duration::from_secs(1),
    )
    .unwrap();

    assert!(!observer.events().contains(&"provider:initial".into()));
    drop(instance);
    assert_eq!(observer.shutdowns(), 1);
}

#[test]
fn direct_target_websockets_must_belong_to_the_contract_loopback_port() {
    assert!(app_target("safe").validate_for_port(9341).is_ok());
    assert!(
        DirectCdpTarget {
            id: "daily".into(),
            page_url: "app://-/index.html".into(),
            websocket_url: "ws://127.0.0.1:9333/devtools/page/daily".into(),
        }
        .validate_for_port(9341)
        .is_err()
    );
    assert!(
        DirectCdpTarget {
            id: "remote".into(),
            page_url: "app://-/index.html".into(),
            websocket_url: "ws://example.com:9341/devtools/page/remote".into(),
        }
        .validate_for_port(9341)
        .is_err()
    );
    assert!(
        DirectCdpTarget {
            id: "wrong-page".into(),
            page_url: "https://example.com".into(),
            websocket_url: "ws://127.0.0.1:9341/devtools/page/wrong-page".into(),
        }
        .validate_for_port(9341)
        .is_err()
    );
}
