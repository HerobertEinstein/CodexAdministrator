#![cfg(windows)]

use std::{fs, net::TcpListener, path::PathBuf, process::Command};

use codex_administrator::{
    DirectInstanceLayout, DirectRuntimeBackend, GrokNativeProviderConfig, WindowsDirectRuntime,
    find_official_chatgpt_executable, validate_official_chatgpt_executable,
};
use tempfile::tempdir;
#[test]
fn windows_runtime_resolves_the_loopback_listener_owner() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("listener-test");
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut runtime = WindowsDirectRuntime::new(root, None).unwrap();

    assert_eq!(runtime.cdp_listener_pid(port).unwrap(), std::process::id());
}

fn powershell_path() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

#[test]
fn windows_runtime_writes_provider_only_to_isolated_codex_home() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("provider-test");
    let daily_profile = temp.path().join("daily-profile");
    let daily_codex_home = temp.path().join("daily-codex-home");
    fs::create_dir_all(&daily_profile).unwrap();
    fs::create_dir_all(&daily_codex_home).unwrap();
    fs::write(
        daily_codex_home.join("config.toml"),
        "model = \"gpt-native\"\n",
    )
    .unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        daily_profile,
        daily_codex_home.clone(),
        9341,
    )
    .unwrap();
    let provider = GrokNativeProviderConfig {
        base_url: "https://api.x.ai/v1".into(),
        env_key: "XAI_API_KEY".into(),
        supports_websockets: false,
    };
    let mut runtime = WindowsDirectRuntime::new(root.clone(), Some(provider)).unwrap();

    runtime.prepare_owned_paths(layout.contract()).unwrap();

    let isolated = fs::read_to_string(root.join("codex-home").join("config.toml")).unwrap();
    assert!(isolated.contains("[model_providers.grok_native]"));
    assert!(isolated.contains("env_key = \"XAI_API_KEY\""));
    assert_eq!(
        fs::read_to_string(daily_codex_home.join("config.toml")).unwrap(),
        "model = \"gpt-native\"\n"
    );
    runtime.shutdown().unwrap();
    assert!(!root.exists());
    assert!(daily_codex_home.exists());
}

#[test]
fn windows_runtime_refuses_a_non_official_executable_before_launch() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("non-official-test");
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();
    let mut runtime = WindowsDirectRuntime::new(root.clone(), None).unwrap();
    runtime.prepare_owned_paths(layout.contract()).unwrap();

    let result = runtime.launch(layout.contract().executable(), &[], &[]);

    assert!(result.is_err());
    assert!(runtime.owned_pids().unwrap().is_empty());
    runtime.shutdown().unwrap();
    assert!(!root.exists());
}

#[test]
fn windows_runtime_refuses_to_prepare_a_nonempty_owned_root() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("occupied");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("unrelated.txt"), "preserve").unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();
    let mut runtime = WindowsDirectRuntime::new(root.clone(), None).unwrap();

    assert!(runtime.prepare_owned_paths(layout.contract()).is_err());
    assert_eq!(
        fs::read_to_string(root.join("unrelated.txt")).unwrap(),
        "preserve"
    );
}

#[test]
fn windows_runtime_rejects_a_reparse_ancestor_without_touching_its_target() {
    let temp = tempdir().unwrap();
    let target = temp.path().join("junction-target");
    let junction = temp.path().join("junction");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("preserve.txt"), "preserve").unwrap();
    let status = Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .status()
        .unwrap();
    assert!(status.success());

    let root = junction
        .join("CodexAdministrator")
        .join("instances")
        .join("reparse-test");
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();
    let mut runtime = WindowsDirectRuntime::new(root, None).unwrap();

    let result = runtime.prepare_owned_paths(layout.contract());
    if result.is_ok() {
        runtime.shutdown().unwrap();
    }

    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(target.join("preserve.txt")).unwrap(),
        "preserve"
    );
    fs::remove_dir(&junction).unwrap();
}

#[test]
fn official_host_validation_accepts_only_the_packaged_chatgpt_executable_shape() {
    let temp = tempdir().unwrap();
    let official = temp
        .path()
        .join("Program Files")
        .join("WindowsApps")
        .join("OpenAI.Codex_test_x64__2p2nqsd0c76g0")
        .join("app")
        .join("ChatGPT.exe");
    fs::create_dir_all(official.parent().unwrap()).unwrap();
    fs::write(&official, b"fixture").unwrap();
    let untrusted = temp.path().join("GrokBuild").join("ChatGPT.exe");
    fs::create_dir_all(untrusted.parent().unwrap()).unwrap();
    fs::write(&untrusted, b"fixture").unwrap();

    validate_official_chatgpt_executable(&official).unwrap();
    assert!(validate_official_chatgpt_executable(&untrusted).is_err());
    assert!(validate_official_chatgpt_executable(&powershell_path()).is_err());
}

#[test]
#[ignore = "requires a running Microsoft Store ChatGPT/Codex instance"]
fn live_official_host_discovery_resolves_the_running_package() {
    let path = find_official_chatgpt_executable().unwrap();
    validate_official_chatgpt_executable(&path).unwrap();
}
