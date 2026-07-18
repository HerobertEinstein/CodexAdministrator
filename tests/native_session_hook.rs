#![cfg(windows)]

use std::{
    collections::BTreeMap,
    env, fs,
    process::{Command, Stdio},
};

use anyhow::Result;
use codex_administrator::{
    NativeSessionHead, NativeSessionHeadStore, NativeTurnCheckpoint, NativeTurnItemCheckpoint,
    NativeTurnStatus, install_native_session_continuity_hook_file,
    observe_native_session_continuity,
    sync_native_session_continuity_hooks_via_official_app_server,
};
use tempfile::tempdir;

#[derive(Default)]
struct FakeHeadStore {
    heads: BTreeMap<String, NativeSessionHead>,
}

impl NativeSessionHeadStore for FakeHeadStore {
    fn read_session_head(&mut self, thread_id: &str) -> Result<NativeSessionHead> {
        Ok(self.heads.get(thread_id).unwrap().clone())
    }
}

#[test]
fn user_prompt_hook_adds_exact_dual_lane_context_without_message_bodies() {
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("session-continuity-manifest.json");
    let thread_id = "019f0000-0000-7000-8000-000000000003";
    let common = turn(
        "turn-common",
        "sha-common",
        NativeTurnStatus::Completed,
        "item-common",
        "agentMessage",
        None,
    );
    let mut daily = FakeHeadStore::default();
    daily.heads.insert(
        thread_id.to_owned(),
        head(
            thread_id,
            "openai",
            vec![
                turn(
                    "turn-daily",
                    "sha-daily",
                    NativeTurnStatus::InProgress,
                    "item-daily-tool",
                    "dynamicToolCall",
                    Some("inProgress"),
                ),
                common.clone(),
            ],
        ),
    );
    let mut isolated = FakeHeadStore::default();
    isolated.heads.insert(
        thread_id.to_owned(),
        head(
            thread_id,
            "grok_native",
            vec![
                turn(
                    "turn-isolated",
                    "sha-isolated",
                    NativeTurnStatus::Interrupted,
                    "item-isolated-final",
                    "agentMessage",
                    None,
                ),
                common,
            ],
        ),
    );
    observe_native_session_continuity(&mut daily, &mut isolated, [thread_id], &manifest).unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["session-continuity-hook", "--lane", "daily", "--manifest"])
        .arg(&manifest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(
        child.stdin.as_mut().unwrap(),
        &serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": thread_id,
            "turn_id": "turn-current-daily",
            "prompt": "private user prompt",
            "cwd": "D:/private/project"
        }),
    )
    .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["continue"], true);
    assert_eq!(value["suppressOutput"], true);
    assert_eq!(
        value["hookSpecificOutput"]["hookEventName"],
        "UserPromptSubmit"
    );
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(context.contains("current_lane=daily"));
    assert!(context.contains("current_turn=turn-current-daily"));
    assert!(context.contains("relation=diverged"));
    assert!(context.contains("common_completed_turn=turn-common"));
    assert!(context.contains("observed_at_unix_ms="));
    assert!(context.contains("observed_age_ms="));
    assert!(context.contains("daily.turn=turn-daily"));
    assert!(context.contains("daily.item=item-daily-tool"));
    assert!(context.contains("isolated.turn=turn-isolated"));
    assert!(context.contains("isolated.item=item-isolated-final"));
    assert!(context.contains("single-writer"));
    assert!(!context.contains("private user prompt"));
    assert!(!context.contains("D:/private/project"));
}

#[test]
fn hook_without_a_continuity_record_is_a_non_blocking_noop() {
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("session-continuity-manifest.json");
    fs::write(&manifest, b"{\"version\":1,\"records\":{}}\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args([
            "session-continuity-hook",
            "--lane",
            "isolated",
            "--manifest",
        ])
        .arg(&manifest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(
        child.stdin.as_mut().unwrap(),
        &serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "019f0000-0000-7000-8000-000000000004",
            "turn_id": "turn-current"
        }),
    )
    .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["continue"], true);
    assert_eq!(value["suppressOutput"], true);
    assert!(value.get("hookSpecificOutput").is_none());
}

#[test]
fn corrupt_continuity_manifest_cannot_block_a_user_prompt() {
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("session-continuity-manifest.json");
    fs::write(&manifest, b"not-json").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["session-continuity-hook", "--lane", "daily", "--manifest"])
        .arg(&manifest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(
        child.stdin.as_mut().unwrap(),
        &serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "019f0000-0000-7000-8000-000000000005",
            "turn_id": "turn-current"
        }),
    )
    .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["continue"], true);
    assert_eq!(value["suppressOutput"], true);
    assert!(value.get("hookSpecificOutput").is_none());
}

#[test]
fn hook_file_installation_preserves_other_hooks_and_is_idempotent() {
    let temp = tempdir().unwrap();
    let home = temp.path().join("codex-home");
    fs::create_dir_all(&home).unwrap();
    let hook_path = home.join("hooks.json");
    fs::write(
        &hook_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "customRootField": "preserve",
            "hooks": {
                "PreToolUse": [{
                    "matcher": "shell_command",
                    "hooks": [{"type": "command", "command": "cmd /c echo pre", "timeout": 5}]
                }],
                "UserPromptSubmit": [{
                    "matcher": "",
                    "hooks": [{"type": "command", "command": "cmd /c echo existing", "timeout": 5}]
                }]
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let command = "powershell.exe -NoProfile -Command continuity session-continuity-hook";

    let first = install_native_session_continuity_hook_file(&home, command).unwrap();
    let second = install_native_session_continuity_hook_file(&home, command).unwrap();

    assert!(first.updated);
    assert!(!second.updated);
    let installed: serde_json::Value =
        serde_json::from_slice(&fs::read(&hook_path).unwrap()).unwrap();
    assert_eq!(installed["customRootField"], "preserve");
    assert_eq!(
        installed["hooks"]["PreToolUse"].as_array().unwrap().len(),
        1
    );
    let user_hooks = installed["hooks"]["UserPromptSubmit"].as_array().unwrap();
    assert_eq!(user_hooks.len(), 2);
    assert_eq!(user_hooks[0]["hooks"][0]["command"], "cmd /c echo existing");
    assert_eq!(user_hooks[1]["hooks"][0]["command"], command);
}

#[test]
#[ignore = "writes hook configuration in two explicitly supplied live Codex homes"]
fn live_official_app_servers_report_both_continuity_hooks_as_trusted() {
    let daily = env::var_os("CODEX_ADMINISTRATOR_TEST_DAILY_HOME")
        .map(std::path::PathBuf::from)
        .expect("set CODEX_ADMINISTRATOR_TEST_DAILY_HOME");
    let isolated = env::var_os("CODEX_ADMINISTRATOR_TEST_ISOLATED_HOME")
        .map(std::path::PathBuf::from)
        .expect("set CODEX_ADMINISTRATOR_TEST_ISOLATED_HOME");
    let command = env::var("CODEX_ADMINISTRATOR_TEST_HOOK_COMMAND")
        .expect("set CODEX_ADMINISTRATOR_TEST_HOOK_COMMAND");

    let receipt =
        sync_native_session_continuity_hooks_via_official_app_server(&daily, &isolated, &command)
            .unwrap()
            .expect("the official npm Codex app-server must be discoverable");

    assert_eq!(receipt.trusted + receipt.already_trusted, 2);
}

fn head(
    thread_id: &str,
    model_provider: &str,
    turns: Vec<NativeTurnCheckpoint>,
) -> NativeSessionHead {
    NativeSessionHead {
        thread_id: thread_id.to_owned(),
        model_provider: model_provider.to_owned(),
        turns,
        history_complete: true,
    }
}

fn turn(
    id: &str,
    fingerprint: &str,
    status: NativeTurnStatus,
    item_id: &str,
    item_type: &str,
    item_status: Option<&str>,
) -> NativeTurnCheckpoint {
    NativeTurnCheckpoint {
        id: id.to_owned(),
        fingerprint: fingerprint.to_owned(),
        status,
        item_count: 1,
        items_complete: true,
        last_item: Some(NativeTurnItemCheckpoint {
            id: item_id.to_owned(),
            item_type: item_type.to_owned(),
            status: item_status.map(str::to_owned),
        }),
    }
}
