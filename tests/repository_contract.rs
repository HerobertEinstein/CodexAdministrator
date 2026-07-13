use std::{fs, path::Path};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn obsolete_alternate_runtime_files_are_absent() {
    for relative in [
        "assets/ui-app.js",
        "assets/ui.html",
        concat!("src/com", "panion.rs"),
        "src/jsonl.rs",
        "src/mode.rs",
        "src/runtime_client.rs",
        "src/runtime_process.rs",
    ] {
        assert!(
            !root().join(relative).exists(),
            "obsolete path remains: {relative}"
        );
    }
}

#[test]
fn active_repository_surfaces_describe_only_model_list_injection() {
    let surfaces = [
        "Cargo.toml",
        "README.md",
        "SECURITY.md",
        "docs/ARCHITECTURE.md",
        "docs/COMPATIBILITY.md",
        "docs/HOST_ADAPTERS.md",
        "docs/UPDATE_ISOLATION.md",
        ".agent_memory/MEMORY.md",
        ".agent_memory/decisions/model-list-injection-boundary.md",
        ".agent_memory/decisions/isolated-official-desktop-instance.md",
        ".agent_memory/decisions/update-isolation-contract.md",
    ];
    let forbidden = [
        concat!("Grok ", "Build"),
        concat!("Grok ", "CLI"),
        concat!("A", "CP"),
        concat!("dual-main", "-agent"),
        concat!("launch", "-native"),
        concat!("com", "panion"),
        concat!("independent Grok ", "UI"),
    ];

    for relative in surfaces {
        let content = fs::read_to_string(root().join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for term in forbidden {
            assert!(
                !content.contains(term),
                "obsolete term {term:?} remains in {relative}"
            );
        }
    }
}

#[test]
fn direct_runtime_binds_the_cdp_listener_to_its_owned_job() {
    let direct = fs::read_to_string(root().join("src/direct.rs")).unwrap();
    let windows = fs::read_to_string(root().join("src/windows_runtime.rs")).unwrap();

    assert!(direct.contains("cdp_listener_pid"));
    assert!(windows.contains("GetExtendedTcpTable"));
    assert!(windows.contains("TCP_TABLE_OWNER_PID_LISTENER"));
}

#[test]
fn direct_runtime_verifies_the_suspended_official_package_before_resume() {
    let windows = fs::read_to_string(root().join("src/windows_runtime.rs")).unwrap();

    assert!(windows.contains("GetPackageFamilyName"));
    assert!(windows.contains("OpenAI.Codex_2p2nqsd0c76g0"));
    assert!(windows.contains("NumberOfAssignedProcesses"));
}

#[test]
fn direct_runtime_does_not_claim_a_tautological_profile_observation() {
    let direct = fs::read_to_string(root().join("src/direct.rs")).unwrap();

    assert!(!direct.contains("observed_profile: contract.isolated_profile()"));
}

#[test]
fn direct_startup_installs_cleanup_signal_handling_before_launch() {
    let main = fs::read_to_string(root().join("src/main.rs")).unwrap();
    let handler = main.find("ctrlc::set_handler").unwrap();
    let startup = main.find("DirectInstance::start").unwrap();

    assert!(handler < startup);
}

#[test]
fn listener_owner_lookup_retries_a_growing_windows_table() {
    let windows = fs::read_to_string(root().join("src/windows_runtime.rs")).unwrap();
    let lookup = windows.split("fn loopback_listener_pid").nth(1).unwrap();

    assert!(lookup.contains("ERROR_INSUFFICIENT_BUFFER"));
    assert!(lookup.contains("continue;"));
}

#[test]
fn doctor_preserves_the_official_host_probe_result() {
    let main = fs::read_to_string(root().join("src/main.rs")).unwrap();
    let doctor = main.split("fn doctor").nth(1).unwrap();

    assert!(doctor.contains("let direct_probe = find_official_chatgpt_executable()"));
    assert!(!doctor.contains("find_official_chatgpt_executable().is_ok()"));
}

#[test]
fn direct_launcher_docs_distinguish_implementation_from_deployment_and_capability_parity() {
    let readme = fs::read_to_string(root().join("README.md")).unwrap();
    let architecture = fs::read_to_string(root().join("docs/ARCHITECTURE.md")).unwrap();
    let adapters = fs::read_to_string(root().join("docs/HOST_ADAPTERS.md")).unwrap();
    let combined = format!("{readme}\n{architecture}\n{adapters}");
    let normalized_readme = readme.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(!combined.contains("isolated launcher is not implemented"));
    assert!(!combined.contains("production launcher, process ownership monitor"));
    assert!(normalized_readme.contains("not released or deployed"));
    assert!(architecture.contains("Windows Job Object"));
    assert!(adapters.contains("UI readiness"));
    assert!(combined.contains("does not prove") || combined.contains("not feature parity"));
}
