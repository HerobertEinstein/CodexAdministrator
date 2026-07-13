use std::{collections::BTreeSet, ffi::OsString, path::PathBuf};

use codex_administrator::{
    DirectInstanceLayout, DirectIsolationContract, IsolatedRuntimeObservation,
};

fn contract() -> DirectIsolationContract {
    DirectIsolationContract::new(
        PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe"),
        PathBuf::from(r"C:\Users\Example\AppData\Roaming\Codex\web\Codex"),
        PathBuf::from(r"C:\Users\Example\AppData\Local\CodexAdministrator\profile"),
        PathBuf::from(r"C:\Users\Example\.codex"),
        PathBuf::from(r"C:\Users\Example\AppData\Local\CodexAdministrator\codex-home"),
        9341,
    )
    .unwrap()
}

#[test]
fn rejects_any_profile_or_codex_home_overlap_with_the_daily_instance() {
    let daily_profile = PathBuf::from(r"C:\Users\Example\AppData\Roaming\Codex\web\Codex");
    let daily_codex_home = PathBuf::from(r"C:\Users\Example\.codex");
    let executable = PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe");

    assert!(
        DirectIsolationContract::new(
            executable.clone(),
            daily_profile.clone(),
            daily_profile.clone(),
            daily_codex_home.clone(),
            PathBuf::from(r"C:\isolated\codex-home"),
            9341,
        )
        .is_err()
    );
    assert!(
        DirectIsolationContract::new(
            executable,
            daily_profile.clone(),
            daily_profile.join("child"),
            daily_codex_home.clone(),
            daily_codex_home.join("child"),
            9341,
        )
        .is_err()
    );
    assert!(
        DirectIsolationContract::new(
            PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe"),
            PathBuf::from(r"\\?\C:\Users\Example\AppData\Roaming\Codex\web\Codex"),
            daily_profile,
            daily_codex_home,
            PathBuf::from(r"C:\isolated\codex-home"),
            9341,
        )
        .is_err()
    );
}

#[test]
fn launch_arguments_and_environment_reference_only_the_isolated_instance() {
    let contract = contract();
    let launch = contract.initial_launch_arguments();
    let activation = contract.activation_arguments();
    let environment = contract.environment_overrides();

    assert!(launch.contains(&OsString::from(
        r"--user-data-dir=C:\Users\Example\AppData\Local\CodexAdministrator\profile"
    )));
    assert!(launch.contains(&OsString::from("--remote-debugging-address=127.0.0.1")));
    assert!(launch.contains(&OsString::from("--remote-debugging-port=9341")));
    assert!(
        !launch
            .iter()
            .any(|value| value.to_string_lossy().contains("Roaming\\Codex"))
    );
    assert_eq!(activation.last(), Some(&OsString::from("--new-window")));
    assert_eq!(
        environment,
        vec![(
            OsString::from("CODEX_HOME"),
            OsString::from(r"C:\Users\Example\AppData\Local\CodexAdministrator\codex-home"),
        )]
    );
}

#[test]
fn runtime_gate_accepts_only_a_disjoint_owned_process_tree_and_isolated_cdp_target() {
    let contract = contract();
    let safe = IsolatedRuntimeObservation {
        cdp_port: 9341,
        cdp_target_url: Some("app://-/index.html".into()),
        daily_root_alive: true,
        owned_pids: BTreeSet::from([400, 401, 402]),
        cdp_listener_pid: 401,
        preexisting_pids: BTreeSet::from([100, 101, 102]),
    };

    contract.verify_runtime(&safe).unwrap();

    let mut overlap = safe.clone();
    overlap.owned_pids.insert(100);
    assert!(contract.verify_runtime(&overlap).is_err());

    let mut daily_closed = safe.clone();
    daily_closed.daily_root_alive = false;
    assert!(contract.verify_runtime(&daily_closed).is_err());

    let mut wrong_target = safe.clone();
    wrong_target.cdp_target_url = Some("app://-/other.html".into());
    assert!(contract.verify_runtime(&wrong_target).is_err());

    let mut foreign_listener = safe.clone();
    foreign_listener.cdp_listener_pid = 999;
    assert!(contract.verify_runtime(&foreign_listener).is_err());
}

#[test]
fn rejects_system_reserved_cdp_ports() {
    let base = contract();

    for port in [0, 80, 443, 1023] {
        assert!(
            DirectIsolationContract::new(
                base.executable().to_path_buf(),
                base.daily_profile().to_path_buf(),
                base.isolated_profile().to_path_buf(),
                base.daily_codex_home().to_path_buf(),
                base.isolated_codex_home().to_path_buf(),
                port,
            )
            .is_err()
        );
    }
}

#[test]
fn instance_layout_owns_only_exact_children_of_a_disjoint_root() {
    let root =
        PathBuf::from(r"C:\Users\Example\AppData\Local\CodexAdministrator\instances\session-1");
    let layout = DirectInstanceLayout::new(
        root.clone(),
        PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe"),
        PathBuf::from(r"C:\Users\Example\AppData\Roaming\Codex\web\Codex"),
        PathBuf::from(r"C:\Users\Example\.codex"),
        9341,
    )
    .unwrap();

    assert_eq!(layout.root(), root);
    assert_eq!(layout.contract().isolated_profile(), root.join("profile"));
    assert_eq!(
        layout.contract().isolated_codex_home(),
        root.join("codex-home")
    );
    layout.verify_contract(layout.contract()).unwrap();
}

#[test]
fn instance_layout_rejects_a_root_that_contains_daily_state() {
    let result = DirectInstanceLayout::new(
        PathBuf::from(r"C:\Users\Example"),
        PathBuf::from(r"C:\Program Files\WindowsApps\OpenAI.Codex\app\ChatGPT.exe"),
        PathBuf::from(r"C:\Users\Example\AppData\Roaming\Codex\web\Codex"),
        PathBuf::from(r"C:\Users\Example\.codex"),
        9341,
    );

    assert!(result.is_err());
}
