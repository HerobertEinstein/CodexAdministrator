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
        ".agent_memory/decisions/native-state-import-boundary.md",
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
    assert!(normalized_readme.contains("not merged or publicly released"));
    assert!(normalized_readme.contains("local project-owned build"));
    assert!(architecture.contains("Windows Job Object"));
    assert!(adapters.contains("UI readiness"));
    assert!(combined.contains("does not prove") || combined.contains("not feature parity"));
}

#[test]
fn local_launcher_docs_distinguish_retained_login_state_from_process_residue() {
    let readme = fs::read_to_string(root().join("README.md")).unwrap();
    let architecture = fs::read_to_string(root().join("docs/ARCHITECTURE.md")).unwrap();
    let launcher =
        fs::read_to_string(root().join("src/bin/codex-administrator-launcher.rs")).unwrap();
    let combined = format!("{readme}\n{architecture}");
    let normalized = combined.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(normalized.contains("persistent isolated profile"));
    assert!(normalized.contains("Native authentication synchronization"));
    assert!(normalized.contains("auth.json"));
    assert!(normalized.contains("exclusive root lock"));
    assert!(normalized.contains("intentional user state, not process residue"));
    assert!(normalized.contains("Windows Credential Manager"));
    assert!(normalized.contains("/v1/models"));
    assert!(normalized.contains("one-way private import"));
    assert!(normalized.contains("disabled by default"));
    assert!(normalized.contains("full prompts, messages, tool output, and environment history"));
    assert!(normalized.contains("session_index.jsonl"));
    assert!(normalized.contains("SQLite/WAL/SHM"));
    assert!(normalized.contains("daily `config.toml`"));
    assert!(normalized.contains("goals, memories"));
    assert!(normalized.contains("CODEX_SQLITE_HOME"));
    assert!(normalized.contains("never overwrite"));
    assert!(normalized.contains("do not prove reliable resume"));
    assert!(normalized.contains("low, medium, and high"));
    assert!(normalized.contains("default is high"));
    assert!(combined.contains("https://docs.x.ai/developers/grok-4-5"));
    assert!(normalized.contains("32,768-token conservative client cap"));
    assert!(normalized.contains("not the provider's official maximum"));
    assert!(launcher.contains("WindowsCredentialStore"));
    assert!(launcher.contains("LauncherSupervisorBackend"));
    assert!(launcher.contains("supervise_launcher"));
    assert!(launcher.contains("spawn_direct_launcher"));
    assert!(!launcher.contains("fetch_model_list"));
    assert!(!launcher.contains("CreateWindowExW"));
    assert!(!launcher.contains("MessageBoxW"));
    assert!(!launcher.contains("CCSwitch"));
    assert!(!launcher.contains("HEBOX_MORE_API_KEY"));
}

#[test]
fn direct_launch_passes_the_reviewed_models_to_the_native_catalog_installer() {
    let main = fs::read_to_string(root().join("src/main.rs")).unwrap();

    assert!(main.contains("new_with_injected_models"));
    assert!(main.contains("new_retained_with_injected_models"));
    assert!(main.contains("new_retained_with_native_state_sync_and_injected_models"));
}

#[test]
fn direct_child_services_the_model_picker_broker_and_emits_a_structured_restart_request() {
    let main = fs::read_to_string(root().join("src/main.rs")).unwrap();

    assert!(main.contains("GrokControlBroker"));
    assert!(main.contains("drain_control_requests"));
    assert!(main.contains("deliver_control_response"));
    assert!(main.contains("restart_requested"));
    assert!(!main.contains("credential.get"));
}

#[test]
fn product_launcher_does_not_depend_on_ccswitch_or_a_fixed_hebox_profile() {
    let root = root();
    let mut content = Vec::new();
    for entry in [
        "Cargo.toml",
        "Cargo.lock",
        "src",
        "assets",
        "scripts",
        "README.md",
        "docs",
    ] {
        collect_text(root.join(entry), &mut content);
    }
    let source = content.join("\n");

    assert!(!source.contains("CCSwitch"));
    assert!(!source.contains("HEBOX_MORE_API_KEY"));
    assert!(!root.join("scripts/launch-hebox-more.ps1").exists());
}

fn collect_text(path: impl AsRef<Path>, content: &mut Vec<String>) {
    let path = path.as_ref();
    if path.is_file() {
        if let Ok(text) = fs::read_to_string(path) {
            content.push(text);
        }
        return;
    }
    if !path.is_dir() {
        return;
    }
    for entry in fs::read_dir(path).unwrap().filter_map(Result::ok) {
        collect_text(entry.path(), content);
    }
}

#[test]
fn product_launcher_is_a_headless_secret_redacting_supervisor() {
    let launcher =
        fs::read_to_string(root().join("src/bin/codex-administrator-launcher.rs")).unwrap();

    assert!(launcher.contains("sanitize_launcher_diagnostic"));
    assert!(launcher.contains("record_fatal_error"));
    assert!(launcher.contains("SupervisorGeneration"));
    assert!(!launcher.contains("EM_SETLIMITTEXT"));
    assert!(!launcher.contains("take_sensitive_text"));
    assert!(!launcher.contains("CreateWindowExW"));
    assert!(!launcher.contains("MessageBoxW"));
    assert!(launcher.contains("std::process::exit(1)"));
}

#[test]
fn public_docs_match_the_headless_launcher_and_current_write_surface() {
    let readme = fs::read_to_string(root().join("README.md")).unwrap();
    let security = fs::read_to_string(root().join("SECURITY.md")).unwrap();
    let update = fs::read_to_string(root().join("docs/UPDATE_ISOLATION.md")).unwrap();
    let architecture = fs::read_to_string(root().join("docs/ARCHITECTURE.md")).unwrap();
    let combined = format!("{readme}\n{security}\n{update}\n{architecture}");
    let normalized = combined.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(!readme.contains("GUI launcher"));
    assert!(normalized.contains("headless supervisor"));
    assert!(security.contains("Windows Credential Manager"));
    assert!(security.contains("renderer addon"));
    for required in [
        "launcher-settings.json",
        "Credential Manager",
        "auth.json",
        "sessions/**/*.jsonl",
        "renderer-addon",
    ] {
        assert!(
            update.contains(required),
            "update-isolation docs omit current write/read surface {required:?}"
        );
    }
    assert!(!normalized.contains(
        "inaccessible, vanishes before opening, or was replaced after the snapshot becomes permanent"
    ));
}

#[test]
fn public_docs_match_the_exact_reviewed_grok_capability_registry() {
    let readme = fs::read_to_string(root().join("README.md")).unwrap();
    let architecture = fs::read_to_string(root().join("docs/ARCHITECTURE.md")).unwrap();
    let combined = format!("{readme}\n{architecture}");

    assert!(!combined.contains("exposes only IDs beginning with `grok-`"));
    for reviewed in [
        "`grok-4.5`",
        "`grok-4.3-{low,medium,high}`",
        "`grok-4.20-multi-agent-{low,medium,high,xhigh}`",
    ] {
        assert!(
            combined.contains(reviewed),
            "public docs omit reviewed model registry entry {reviewed}"
        );
    }
    assert!(combined.contains("Unreviewed Grok IDs are not injected"));
}

#[test]
fn public_docs_record_current_control_and_state_safety_boundaries() {
    let readme = fs::read_to_string(root().join("README.md")).unwrap();
    let security = fs::read_to_string(root().join("SECURITY.md")).unwrap();
    let architecture = fs::read_to_string(root().join("docs/ARCHITECTURE.md")).unwrap();
    let update = fs::read_to_string(root().join("docs/UPDATE_ISOLATION.md")).unwrap();
    let combined = format!("{readme}\n{security}\n{architecture}\n{update}");
    let normalized = combined.split_whitespace().collect::<Vec<_>>().join(" ");

    for required in [
        "Changing the Base URL or Action Path requires a fresh API key",
        "Management-only startup never passes the provider key to the official child",
        "A control request that times out while queued is invalidated before it can be drained",
        "Hard-linked `auth.json` and task snapshots are rejected",
        "Existing tool configuration is preserved",
        "The stored credential includes an endpoint fingerprint",
        "secret-shaped inherited environment variables",
        "FILE_FLAG_OPEN_REPARSE_POINT",
        "Provider cleanup retains the shell exclusion",
    ] {
        assert!(
            normalized.contains(required),
            "public docs omit current safety boundary {required:?}"
        );
    }
}

#[test]
fn public_docs_keep_transient_runtime_results_dated_and_non_current() {
    let surfaces = [
        "README.md",
        "docs/ARCHITECTURE.md",
        "docs/COMPATIBILITY.md",
        "docs/HOST_ADAPTERS.md",
    ];
    let mut combined = String::new();
    for relative in surfaces {
        combined.push_str(&fs::read_to_string(root().join(relative)).unwrap());
        combined.push('\n');
    }

    for stale in [
        "26.707.12708.0",
        "26.707.9981.0",
        "HTTP 422",
        "HTTP 503",
        "HE BOX more",
        "hebox-more",
    ] {
        assert!(
            !combined.contains(stale),
            "public docs retain a stale or private operational detail: {stale}"
        );
    }
    assert!(combined.contains("2026-07-17"));
    assert!(combined.contains("26.715.2305.0"));
}

#[test]
fn public_docs_and_memory_do_not_overstate_disabled_or_internal_surfaces() {
    let readme = fs::read_to_string(root().join("README.md")).unwrap();
    let architecture = fs::read_to_string(root().join("docs/ARCHITECTURE.md")).unwrap();
    let adapters = fs::read_to_string(root().join("docs/HOST_ADAPTERS.md")).unwrap();
    let multi = fs::read_to_string(
        root().join(".agent_memory/decisions/multi-injector-composition-boundary.md"),
    )
    .unwrap();
    let isolated = fs::read_to_string(
        root().join(".agent_memory/decisions/isolated-official-desktop-instance.md"),
    )
    .unwrap();
    let model_boundary =
        fs::read_to_string(root().join(".agent_memory/decisions/model-list-injection-boundary.md"))
            .unwrap();
    let combined = format!("{readme}\n{architecture}\n{adapters}");
    let normalized = combined.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(normalized.contains("No Codex++ executable is currently eligible"));
    assert!(!readme.contains("--host codexplusplus"));
    assert!(readme.contains("https://github.com/Fei-Away/Codex-Dream-Skin"));
    assert!(normalized.contains("editable first-run default"));
    assert!(!architecture.contains("this release honored it"));
    assert!(!combined.contains("current-package startup"));
    assert!(!architecture.contains("Codex++ 1.2.34"));
    for stale in [
        "195 passed",
        "52 passed",
        "HEBOX_DESKTOP_SHELL_TOOL_OK",
        "HEBOX_DESKTOP_SHELL_FINAL_OK",
        "later r7",
        "still requires fresh full gates",
    ] {
        assert!(
            !format!("{multi}\n{isolated}\n{model_boundary}").contains(stale),
            "public project memory retains internal or stale evidence {stale:?}"
        );
    }
}

#[test]
fn unverified_codexplusplus_does_not_receive_renderer_addons() {
    let compatibility: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root().join("compatibility.json")).unwrap())
            .unwrap();
    assert_eq!(compatibility["hosts"].as_array().unwrap().len(), 0);

    let addons: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root().join("renderer-addons.json")).unwrap())
            .unwrap();
    for addon in addons["addons"].as_array().unwrap() {
        let adapters = addon["host_adapters"].as_array().unwrap();
        assert!(
            adapters
                .iter()
                .all(|adapter| adapter.as_str() != Some("codexplusplus")),
            "an addon is enabled for Codex++ before the host compatibility gate"
        );
    }
}

#[test]
fn local_verification_evidence_is_not_part_of_the_public_repository_surface() {
    let ignore = fs::read_to_string(root().join(".gitignore")).unwrap();
    let memory = fs::read_to_string(root().join(".agent_memory/MEMORY.md")).unwrap();

    assert!(
        ignore
            .lines()
            .any(|line| line == "/.agent_memory/verification/")
    );
    assert!(!memory.contains("verification/"));
}

#[test]
fn github_and_contributor_gates_use_locked_release_commands_on_every_push() {
    let ci = fs::read_to_string(root().join(".github/workflows/ci.yml")).unwrap();
    let contributing = fs::read_to_string(root().join("CONTRIBUTING.md")).unwrap();
    let readme = fs::read_to_string(root().join("README.md")).unwrap();

    assert!(!ci.contains("branches: [main]"));
    for command in [
        "cargo check --all-targets --locked",
        "cargo test --all-targets --locked",
        "cargo clippy --all-targets --all-features --locked -- -D warnings",
        "cargo build --release --locked",
        "node --test tests/*.test.mjs",
    ] {
        assert!(ci.contains(command), "CI omits release gate {command:?}");
    }
    for command in [
        "cargo check --all-targets --locked",
        "cargo test --all-targets --locked",
        "cargo clippy --all-targets --all-features --locked -- -D warnings",
        "cargo build --release --locked",
        "node --test tests/*.test.mjs",
    ] {
        assert!(
            contributing.contains(command),
            "contributor docs omit release gate {command:?}"
        );
        assert!(
            readme.contains(command),
            "README development commands omit release gate {command:?}"
        );
    }
    assert!(contributing.contains("Before every push, pull request, merge, tag, or release"));
    assert!(contributing.contains("keep the work unpublished"));
}

#[test]
fn windows_job_cleanup_allocates_its_timeout_after_initial_capture() {
    let windows = fs::read_to_string(root().join("src/windows_runtime.rs")).unwrap();
    let terminate = windows
        .split("fn terminate(mut self)")
        .nth(1)
        .unwrap()
        .split("impl Drop for OwnedJob")
        .next()
        .unwrap();
    let drop_cleanup = windows
        .split("impl Drop for OwnedJob")
        .nth(1)
        .unwrap()
        .split("struct CaptureOutcome")
        .next()
        .unwrap();

    assert!(
        terminate.find("capture_descendants(true)").unwrap()
            < terminate.find("let deadline").unwrap(),
        "explicit cleanup starts its deadline before the initial process capture"
    );
    assert!(terminate.contains("DESCENDANT_CLEANUP_TIMEOUT"));
    assert!(
        drop_cleanup.find("capture_descendants(true)").unwrap()
            < drop_cleanup.find("let deadline").unwrap(),
        "drop cleanup starts its deadline before the initial process capture"
    );
    assert!(drop_cleanup.contains("DESCENDANT_CLEANUP_TIMEOUT"));
}

#[test]
fn security_policy_states_the_unauthenticated_local_cdp_boundary() {
    let security = fs::read_to_string(root().join("SECURITY.md")).unwrap();
    let normalized = security.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(normalized.contains("Chromium DevTools endpoint is unauthenticated"));
    assert!(normalized.contains("hostile local process"));
    assert!(normalized.contains("random loopback port"));
}

#[test]
fn public_docs_match_the_current_cleanup_budget_and_dated_package_evidence() {
    let architecture = fs::read_to_string(root().join("docs/ARCHITECTURE.md")).unwrap();
    let compatibility = fs::read_to_string(root().join("docs/COMPATIBILITY.md")).unwrap();
    let adapters = fs::read_to_string(root().join("docs/HOST_ADAPTERS.md")).unwrap();
    let memory = fs::read_to_string(
        root().join(".agent_memory/decisions/isolated-official-desktop-instance.md"),
    )
    .unwrap();
    let combined = format!("{architecture}\n{compatibility}\n{adapters}\n{memory}");
    let normalized = combined.split_whitespace().collect::<Vec<_>>().join(" ");

    for stale in [
        "ten-second absolute deadline",
        "one ten-second deadline",
        "bounded to ten seconds",
        "begins before the initial global scan",
        "start one ten-second absolute deadline before their initial global scan",
    ] {
        assert!(
            !normalized.contains(stale),
            "cleanup docs retain stale timing claim {stale:?}"
        );
    }
    assert!(normalized.contains("thirty-second descendant cleanup budget"));
    assert!(normalized.contains("after the initial global scan"));
    assert!(!compatibility.contains("same dated package"));
    assert!(compatibility.contains("earlier dated package"));
}

#[test]
fn native_auth_sync_uses_one_bounded_identity_checked_handle() {
    let windows = fs::read_to_string(root().join("src/windows_runtime.rs")).unwrap();
    let sync = windows
        .split("fn sync_native_auth_state")
        .nth(1)
        .unwrap()
        .split("fn json_contains_string_fragment")
        .next()
        .unwrap();

    assert!(!sync.contains("fs::read(source)"));
    assert!(sync.contains("FILE_FLAG_OPEN_REPARSE_POINT"));
    assert!(sync.contains("GetFileInformationByHandle"));
    assert!(sync.contains("nNumberOfLinks"));
    assert!(sync.contains("take(MAX_NATIVE_AUTH_BYTES + 1)"));
}
