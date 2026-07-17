#![cfg(windows)]

use std::{fs, net::TcpListener, path::PathBuf, process::Command, thread, time::Duration};

use codex_administrator::{
    DirectInstanceLayout, DirectRuntimeBackend, GrokNativeProviderConfig, InjectedModelDescriptor,
    WindowsDirectRuntime, find_installed_official_chatgpt_executable,
    find_official_chatgpt_executable, select_latest_official_package_candidate,
    validate_official_chatgpt_executable,
};
use serde_json::json;
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

#[test]
fn provider_readiness_is_required_only_for_configured_runtimes() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("runtime");
    let management = WindowsDirectRuntime::new(root.join("management"), None).unwrap();
    let configured = WindowsDirectRuntime::new(
        root.join("configured"),
        Some(GrokNativeProviderConfig {
            base_url: "https://ai.hebox.net/v1".into(),
            action_path: "/responses".into(),
            env_key: "TEST_GROK_KEY".into(),
            supports_websockets: false,
        }),
    )
    .unwrap();

    assert!(!management.requires_provider_readiness());
    assert!(configured.requires_provider_readiness());
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
        action_path: "/responses".into(),
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
fn windows_runtime_installs_the_official_catalog_overlay_for_injected_models() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("catalog-test");
    let app = temp.path().join("official").join("app");
    let executable = app.join("ChatGPT.exe");
    let codex_binary = app.join("resources").join("codex.exe");
    fs::create_dir_all(codex_binary.parent().unwrap()).unwrap();
    fs::write(&executable, b"fixture").unwrap();
    let mut binary = b"official-prefix\0".to_vec();
    binary.extend(
        serde_json::to_vec_pretty(&json!({
            "models": [{
                "slug": "gpt-native",
                "base_instructions": "OFFICIAL_NATIVE_INSTRUCTIONS",
                "context_window": 272000,
                "max_context_window": 272000,
                "effective_context_window_percent": 95
            }]
        }))
        .unwrap(),
    );
    binary.extend(b"\0official-suffix");
    fs::write(&codex_binary, binary).unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        executable,
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();
    let provider = GrokNativeProviderConfig {
        base_url: "https://api.x.ai/v1".into(),
        action_path: "/responses".into(),
        env_key: "XAI_API_KEY".into(),
        supports_websockets: false,
    };
    let mut runtime = WindowsDirectRuntime::new_with_injected_models(
        root.clone(),
        Some(provider),
        vec![InjectedModelDescriptor::grok("grok-4.5")],
    )
    .unwrap();

    runtime.prepare_owned_paths(layout.contract()).unwrap();

    let catalog_path = root
        .join("codex-home")
        .join("grok-native-model-catalog.json");
    let catalog: serde_json::Value =
        serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
    assert_eq!(catalog["models"][0]["slug"], "gpt-native");
    assert_eq!(catalog["models"][1]["slug"], "grok-4.5");
    let config = fs::read_to_string(root.join("codex-home").join("config.toml")).unwrap();
    assert!(config.contains("model_catalog_json"));
    assert!(config.contains("[model_providers.grok_native]"));
    runtime.shutdown().unwrap();
}

#[test]
fn retained_management_runtime_removes_stale_provider_and_catalog_state() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("management-cleanup");
    let app = temp.path().join("official").join("app");
    let executable = app.join("ChatGPT.exe");
    let codex_binary = app.join("resources").join("codex.exe");
    fs::create_dir_all(codex_binary.parent().unwrap()).unwrap();
    fs::write(&executable, b"fixture").unwrap();
    let mut binary = b"official-prefix\0".to_vec();
    binary.extend(
        serde_json::to_vec_pretty(&json!({
            "models": [{
                "slug": "gpt-native",
                "base_instructions": "OFFICIAL_NATIVE_INSTRUCTIONS",
                "context_window": 272000,
                "max_context_window": 272000,
                "effective_context_window_percent": 95
            }]
        }))
        .unwrap(),
    );
    fs::write(&codex_binary, binary).unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        executable,
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();
    let provider = GrokNativeProviderConfig {
        base_url: "https://api.x.ai/v1".into(),
        action_path: "/responses".into(),
        env_key: "XAI_API_KEY".into(),
        supports_websockets: false,
    };

    let mut configured = WindowsDirectRuntime::new_retained_with_injected_models(
        root.clone(),
        Some(provider),
        vec![InjectedModelDescriptor::grok("grok-4.5")],
    )
    .unwrap();
    configured.prepare_owned_paths(layout.contract()).unwrap();
    configured.shutdown().unwrap();
    assert!(
        fs::read_to_string(root.join("codex-home").join("config.toml"))
            .unwrap()
            .contains("model_providers.grok_native")
    );

    let mut management = WindowsDirectRuntime::new_retained(root.clone(), None).unwrap();
    management.prepare_owned_paths(layout.contract()).unwrap();
    let config = fs::read_to_string(root.join("codex-home").join("config.toml")).unwrap();
    assert!(!config.contains("model_providers.grok_native"));
    assert!(!config.contains("model_catalog_json"));
    assert!(
        !root
            .join("codex-home")
            .join("grok-native-model-catalog.json")
            .exists()
    );
    management.shutdown().unwrap();
}

#[test]
fn retained_windows_runtime_reuses_its_isolated_profile_after_shutdown() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("retained-profile");
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();

    let mut first = WindowsDirectRuntime::new_retained(root.clone(), None).unwrap();
    first.prepare_owned_paths(layout.contract()).unwrap();
    let marker = root.join("profile").join("login-state.test");
    fs::write(&marker, "signed-in").unwrap();
    first.shutdown().unwrap();

    assert_eq!(fs::read_to_string(&marker).unwrap(), "signed-in");

    let mut second = WindowsDirectRuntime::new_retained(root.clone(), None).unwrap();
    second.prepare_owned_paths(layout.contract()).unwrap();
    assert_eq!(fs::read_to_string(&marker).unwrap(), "signed-in");
    second.shutdown().unwrap();
    assert!(root.exists());
}

#[test]
fn retained_windows_runtime_syncs_the_daily_native_auth_state() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("retained-native-auth");
    let daily_codex_home = temp.path().join("daily-codex-home");
    fs::create_dir_all(&daily_codex_home).unwrap();
    let native_auth = br#"{"OPENAI_API_KEY":"native-login-key"}"#;
    fs::write(daily_codex_home.join("auth.json"), native_auth).unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        daily_codex_home.clone(),
        9341,
    )
    .unwrap();

    let mut runtime =
        WindowsDirectRuntime::new_retained_with_native_auth_sync(root.clone(), None).unwrap();
    runtime.prepare_owned_paths(layout.contract()).unwrap();

    assert_eq!(
        fs::read(root.join("codex-home").join("auth.json")).unwrap(),
        native_auth
    );
    assert_eq!(
        fs::read(daily_codex_home.join("auth.json")).unwrap(),
        native_auth
    );
    runtime.shutdown().unwrap();
}

#[test]
fn retained_windows_runtime_rejects_hard_linked_daily_auth_state() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("retained-hard-linked-auth");
    let daily_codex_home = temp.path().join("daily-codex-home");
    fs::create_dir_all(&daily_codex_home).unwrap();
    let auth = daily_codex_home.join("auth.json");
    fs::write(&auth, br#"{"OPENAI_API_KEY":"native-login-key"}"#).unwrap();
    fs::hard_link(&auth, temp.path().join("auth-hard-link.json")).unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        daily_codex_home,
        9341,
    )
    .unwrap();

    let mut runtime =
        WindowsDirectRuntime::new_retained_with_native_auth_sync(root.clone(), None).unwrap();
    let error = runtime.prepare_owned_paths(layout.contract()).unwrap_err();

    assert!(error.to_string().contains("hard link"));
    assert!(!root.join("codex-home").join("auth.json").exists());
}

#[test]
fn installed_official_package_selection_uses_the_highest_four_part_version() {
    let candidates = [
        PathBuf::from(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.707.9981.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
        ),
        PathBuf::from(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_26.710.100.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
        ),
        PathBuf::from(
            r"C:\Program Files\WindowsApps\OpenAI.Codex_invalid_x64__2p2nqsd0c76g0\app\ChatGPT.exe",
        ),
    ];

    assert_eq!(
        select_latest_official_package_candidate(candidates.iter()),
        Some(candidates[1].clone())
    );
}

#[test]
fn retained_windows_runtime_imports_stable_daily_sessions_into_its_private_home() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("retained-native-sessions");
    let daily_codex_home = temp.path().join("daily-codex-home");
    let thread_id = "019f0000-0000-7000-8000-000000000010";
    let source = daily_codex_home.join(format!(
        "sessions/2026/07/15/rollout-2026-07-15T00-00-00-{thread_id}.jsonl"
    ));
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(
        &source,
        format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{thread_id}\",\"model_provider\":\"hebox\"}}}}\n"
        ),
    )
    .unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        daily_codex_home.clone(),
        9341,
    )
    .unwrap();

    let mut runtime =
        WindowsDirectRuntime::new_retained_with_native_state_sync_and_injected_models(
            root.clone(),
            None,
            Vec::new(),
            false,
            true,
            false,
        )
        .unwrap();
    runtime.prepare_owned_paths(layout.contract()).unwrap();

    let imported = root.join(format!(
        "codex-home/sessions/2026/07/15/rollout-2026-07-15T00-00-00-{thread_id}.jsonl"
    ));
    assert!(imported.is_file());
    assert!(
        fs::read_to_string(imported)
            .unwrap()
            .contains("\"model_provider\":\"grok_native\"")
    );
    assert!(root.join("codex-home/sqlite").is_dir());
    assert!(
        fs::read_to_string(root.join("codex-home/config.toml"))
            .unwrap()
            .contains("sqlite_home")
    );
    assert!(!root.join("codex-home/state_5.sqlite").exists());
    runtime.shutdown().unwrap();
}

#[test]
fn retained_windows_runtime_projects_daily_custom_skills_on_launch() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("retained-native-skills");
    let daily_codex_home = temp.path().join("daily-codex-home");
    fs::create_dir_all(daily_codex_home.join("skills/custom-skill")).unwrap();
    fs::create_dir_all(daily_codex_home.join("skills/.system")).unwrap();
    fs::write(
        daily_codex_home.join("skills/custom-skill/SKILL.md"),
        "# Custom skill\n",
    )
    .unwrap();
    fs::write(
        daily_codex_home.join("skills/.system/SKILL.md"),
        "# Daily system skill\n",
    )
    .unwrap();
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        daily_codex_home,
        9341,
    )
    .unwrap();
    let mut runtime =
        WindowsDirectRuntime::new_retained_with_native_state_sync_and_injected_models(
            root.clone(),
            None,
            Vec::new(),
            false,
            false,
            true,
        )
        .unwrap();

    runtime.prepare_owned_paths(layout.contract()).unwrap();

    assert_eq!(
        fs::read_to_string(root.join("codex-home/skills/custom-skill/SKILL.md")).unwrap(),
        "# Custom skill\n"
    );
    assert!(!root.join("codex-home/skills/.system/SKILL.md").exists());
    assert!(
        root.join("codex-home/skill-projection-manifest.json")
            .is_file()
    );
    runtime.shutdown().unwrap();
}

#[test]
fn retained_windows_runtime_refuses_a_concurrent_owner() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("retained-lock");
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();
    let mut first = WindowsDirectRuntime::new_retained(root.clone(), None).unwrap();
    first.prepare_owned_paths(layout.contract()).unwrap();

    let mut second = WindowsDirectRuntime::new_retained(root.clone(), None).unwrap();
    assert!(second.prepare_owned_paths(layout.contract()).is_err());

    first.shutdown().unwrap();
    second.prepare_owned_paths(layout.contract()).unwrap();
    second.shutdown().unwrap();
}

#[test]
fn retained_windows_runtime_cleans_a_stale_exact_root_process_before_reuse() {
    let temp = tempdir().unwrap();
    let root = temp
        .path()
        .join("CodexAdministrator")
        .join("instances")
        .join("retained-stale-process");
    let layout = DirectInstanceLayout::new(
        root.clone(),
        powershell_path(),
        temp.path().join("daily-profile"),
        temp.path().join("daily-codex-home"),
        9341,
    )
    .unwrap();
    let mut first = WindowsDirectRuntime::new_retained(root.clone(), None).unwrap();
    first.prepare_owned_paths(layout.contract()).unwrap();
    first.shutdown().unwrap();

    let script = root.join("stale.ps1");
    fs::write(&script, "Start-Sleep -Seconds 30\n").unwrap();
    let mut stale = Command::new(powershell_path())
        .args(["-NoLogo", "-NoProfile", "-File"])
        .arg(&script)
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(300));

    let mut second = WindowsDirectRuntime::new_retained(root.clone(), None).unwrap();
    let prepare = second.prepare_owned_paths(layout.contract());
    let stale_exited = stale.try_wait().unwrap().is_some();
    if !stale_exited {
        stale.kill().unwrap();
        stale.wait().unwrap();
    }

    prepare.unwrap();
    assert!(stale_exited);
    second.shutdown().unwrap();
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

#[test]
#[ignore = "requires access to the installed Microsoft Store package root"]
fn live_installed_package_fallback_resolves_without_process_discovery() {
    let path = find_installed_official_chatgpt_executable().unwrap();
    validate_official_chatgpt_executable(&path).unwrap();
}
