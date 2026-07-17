#![cfg(windows)]

use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::fs::{OpenOptionsExt, symlink_file},
};

use codex_administrator::{install_isolated_sqlite_home, sync_native_session_snapshots};
use filetime::FileTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

const THREAD_ID: &str = "019f0000-0000-7000-8000-000000000001";

fn rollout(first_provider: &str) -> String {
    format!(
        "{{\"timestamp\":\"2026-07-15T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{THREAD_ID}\",\"timestamp\":\"2026-07-15T00:00:00Z\",\"cwd\":\"D:\\\\Projects\\\\Example\",\"model_provider\":\"{first_provider}\"}}}}\n{{\"timestamp\":\"2026-07-15T00:01:00Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[]}}}}\n{{\"timestamp\":\"2026-07-15T00:02:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"fork-history-id\",\"model_provider\":\"keep-me\"}}}}\n"
    )
}

#[test]
fn stable_daily_rollouts_are_atomically_imported_without_copying_sqlite() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(&relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, rollout("hebox")).unwrap();
    fs::write(
        daily.join("session_index.jsonl"),
        format!(
            "{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"原生会话名称\",\"updated_at\":\"2026-07-15T00:03:00Z\"}}\n"
        ),
    )
    .unwrap();
    for forbidden in [
        "state_5.sqlite",
        "state_5.sqlite-wal",
        "state_5.sqlite-shm",
        "logs_2.sqlite",
        "goals_1.sqlite",
        "memories_1.sqlite",
        "config.toml",
    ] {
        fs::write(daily.join(forbidden), format!("must-not-copy-{forbidden}")).unwrap();
    }
    let source_hash = format!("{:x}", Sha256::digest(fs::read(&source).unwrap()));
    let source_time = FileTime::from_last_modification_time(&fs::metadata(&source).unwrap());

    let receipt = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(receipt.imported, 1);
    assert_eq!(receipt.updated, 0);
    assert_eq!(receipt.conflicts, 0);

    let destination = isolated.join(&relative);
    let lines = fs::read_to_string(&destination)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines[0]["payload"]["model_provider"], "grok_native");
    assert_eq!(lines[1]["type"], "response_item");
    assert_eq!(lines[2]["payload"]["model_provider"], "keep-me");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&source).unwrap())),
        source_hash
    );
    assert_eq!(
        FileTime::from_last_modification_time(&fs::metadata(&destination).unwrap()),
        source_time
    );
    for forbidden in [
        "state_5.sqlite",
        "state_5.sqlite-wal",
        "state_5.sqlite-shm",
        "logs_2.sqlite",
        "goals_1.sqlite",
        "memories_1.sqlite",
        "config.toml",
    ] {
        assert!(!isolated.join(forbidden).exists(), "copied {forbidden}");
    }
    assert!(isolated.join("session-import-manifest.json").is_file());
    assert!(
        fs::read_to_string(isolated.join("session_index.jsonl"))
            .unwrap()
            .contains("原生会话名称")
    );

    let second = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(second.unchanged, 1);
}

#[test]
fn a_published_update_with_an_old_manifest_is_recovered() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(&relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, rollout("hebox")).unwrap();

    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    let manifest = isolated.join("session-import-manifest.json");
    let old_manifest = fs::read(&manifest).unwrap();
    fs::write(
        &source,
        rollout("hebox").replace("\"role\":\"user\"", "\"role\":\"tool\""),
    )
    .unwrap();
    let updated = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(updated.updated, 1);
    fs::write(&manifest, old_manifest).unwrap();

    let recovered = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(recovered.conflicts, 0);
    assert_eq!(recovered.updated, 1);
    assert!(
        fs::read_to_string(isolated.join(relative))
            .unwrap()
            .contains("\"role\":\"tool\"")
    );
}

#[test]
fn same_length_source_changes_with_restored_mtime_are_imported() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(&relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, rollout("hebox")).unwrap();
    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    let original_time = FileTime::from_last_modification_time(&fs::metadata(&source).unwrap());

    let changed = rollout("hebox").replace("\"role\":\"user\"", "\"role\":\"tool\"");
    assert_eq!(changed.len(), rollout("hebox").len());
    fs::write(&source, changed).unwrap();
    filetime::set_file_mtime(&source, original_time).unwrap();

    let receipt = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(receipt.updated, 1);
    assert_eq!(receipt.unchanged, 0);
    assert!(
        fs::read_to_string(isolated.join(relative))
            .unwrap()
            .contains("\"role\":\"tool\"")
    );
}

#[test]
fn same_length_private_changes_with_restored_mtime_remain_conflicts() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(&relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, rollout("hebox")).unwrap();
    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();

    let destination = isolated.join(&relative);
    let original_time = FileTime::from_last_modification_time(&fs::metadata(&destination).unwrap());
    let local = fs::read_to_string(&destination)
        .unwrap()
        .replace("\"role\":\"user\"", "\"role\":\"tool\"");
    fs::write(&destination, &local).unwrap();
    filetime::set_file_mtime(&destination, original_time).unwrap();

    let receipt = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(receipt.conflicts, 1);
    assert_eq!(fs::read_to_string(destination).unwrap(), local);
}

#[test]
fn session_index_uses_last_entry_and_real_timestamp_ordering() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(source, rollout("hebox")).unwrap();
    fs::write(
        daily.join("session_index.jsonl"),
        format!(
            "{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"日常旧行\",\"updated_at\":\"2026-07-15T00:00:00Z\"}}\n{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"日常最后一行\",\"updated_at\":\"2026-07-15T00:00:00Z\"}}\n"
        ),
    )
    .unwrap();
    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    fs::write(
        isolated.join("session_index.jsonl"),
        format!(
            "{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"隔离更新名称\",\"updated_at\":\"2026-07-15T00:00:00.500Z\"}}\n"
        ),
    )
    .unwrap();

    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    let merged = fs::read_to_string(isolated.join("session_index.jsonl")).unwrap();
    assert!(merged.contains("隔离更新名称"));
    assert!(!merged.contains("日常旧行"));
    assert!(!merged.contains("日常最后一行"));
}

#[test]
fn invalid_archived_duplicate_does_not_mask_a_valid_active_rollout() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let active = daily.join(format!(
        "sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl"
    ));
    let archived = daily.join(format!(
        "archived_sessions/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl"
    ));
    fs::create_dir_all(active.parent().unwrap()).unwrap();
    fs::create_dir_all(archived.parent().unwrap()).unwrap();
    fs::write(&active, rollout("hebox")).unwrap();
    fs::write(&archived, []).unwrap();

    let receipt = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(receipt.imported, 1);
    assert_eq!(receipt.conflicts, 0);
    assert!(
        isolated
            .join(format!(
                "sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl"
            ))
            .is_file()
    );
}

#[test]
fn newer_private_session_name_wins_without_writing_back_to_daily_state() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(source, rollout("hebox")).unwrap();
    let daily_index = daily.join("session_index.jsonl");
    fs::write(
        &daily_index,
        format!(
            "{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"日常名称\",\"updated_at\":\"2026-07-15T00:01:00Z\"}}\n"
        ),
    )
    .unwrap();
    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();

    fs::write(
        isolated.join("session_index.jsonl"),
        format!(
            "{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"隔离实例名称\",\"updated_at\":\"2026-07-15T00:02:00Z\"}}\n"
        ),
    )
    .unwrap();
    let daily_before = fs::read(&daily_index).unwrap();
    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();

    let isolated_index = fs::read_to_string(isolated.join("session_index.jsonl")).unwrap();
    assert!(isolated_index.contains("隔离实例名称"));
    assert!(!isolated_index.contains("日常名称"));
    assert_eq!(fs::read(daily_index).unwrap(), daily_before);
}

#[test]
fn active_invalid_and_locally_modified_rollouts_fail_closed() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let sessions = daily.join("sessions/2026/07/15");
    fs::create_dir_all(&sessions).unwrap();

    let active_id = "019f0000-0000-7000-8000-000000000002";
    let active = sessions.join(format!("rollout-2026-07-15T00-00-00-{active_id}.jsonl"));
    fs::write(&active, rollout("hebox").replace(THREAD_ID, active_id)).unwrap();
    let _lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&active)
        .unwrap();

    let invalid_id = "019f0000-0000-7000-8000-000000000003";
    let invalid = sessions.join(format!("rollout-2026-07-15T00-00-00-{invalid_id}.jsonl"));
    fs::write(
        &invalid,
        rollout("hebox")
            .replace(THREAD_ID, invalid_id)
            .trim_end_matches('\n'),
    )
    .unwrap();

    let receipt = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(receipt.skipped_active, 1);
    assert_eq!(receipt.skipped_invalid, 1);

    drop(_lock);
    let imported = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(imported.imported, 1);
    let destination = isolated.join(format!(
        "sessions/2026/07/15/rollout-2026-07-15T00-00-00-{active_id}.jsonl"
    ));
    let mut local = OpenOptions::new().append(true).open(&destination).unwrap();
    writeln!(local, "{{\"type\":\"event_msg\",\"payload\":{{}}}}").unwrap();
    drop(local);
    fs::write(
        &active,
        rollout("hebox-updated").replace(THREAD_ID, active_id),
    )
    .unwrap();

    let conflict = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(conflict.conflicts, 1);
    assert!(
        fs::read_to_string(destination)
            .unwrap()
            .contains("event_msg")
    );
}

#[test]
fn hard_linked_daily_rollouts_are_rejected() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(&relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, rollout("hebox")).unwrap();
    fs::hard_link(&source, temp.path().join("rollout-hard-link.jsonl")).unwrap();

    let receipt = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();

    assert_eq!(receipt.skipped_invalid, 1);
    assert!(!isolated.join(relative).exists());
}

#[test]
fn reparse_backed_daily_session_index_is_rejected() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    fs::create_dir_all(&daily).unwrap();
    let external = temp.path().join("external-session-index.jsonl");
    fs::write(
        &external,
        format!(
            "{{\"id\":\"{THREAD_ID}\",\"thread_name\":\"external\",\"updated_at\":\"2026-07-17T00:00:00Z\"}}\n"
        ),
    )
    .unwrap();
    symlink_file(&external, daily.join("session_index.jsonl")).unwrap();

    let receipt = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();

    assert!(receipt.session_index_skipped);
    assert!(!isolated.join("session_index.jsonl").exists());
}

#[test]
fn published_rollout_without_manifest_is_verified_and_recovered() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let relative = format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(&relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, rollout("hebox")).unwrap();

    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    fs::remove_file(isolated.join("session-import-manifest.json")).unwrap();

    let recovered = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(recovered.conflicts, 0);
    assert_eq!(recovered.unchanged, 1);
    assert!(isolated.join("session-import-manifest.json").is_file());
    assert!(isolated.join(relative).is_file());
}

#[test]
fn archived_rollout_migration_keeps_one_private_copy_per_thread() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let active_relative =
        format!("sessions/2026/07/15/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let archived_relative =
        format!("archived_sessions/rollout-2026-07-15T00-00-00-{THREAD_ID}.jsonl");
    let source = daily.join(&active_relative);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, rollout("hebox")).unwrap();
    sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();

    let archived = daily.join(&archived_relative);
    fs::create_dir_all(archived.parent().unwrap()).unwrap();
    fs::rename(&source, &archived).unwrap();

    let migrated = sync_native_session_snapshots(&daily, &isolated, "grok_native").unwrap();
    assert_eq!(migrated.conflicts, 0);
    assert_eq!(migrated.updated, 1);
    assert!(!isolated.join(active_relative).exists());
    assert!(isolated.join(archived_relative).is_file());
}

#[test]
fn isolated_sqlite_home_is_written_inside_the_owned_codex_home() {
    let temp = tempdir().unwrap();
    let codex_home = temp.path().join("codex-home");
    let config = codex_home.join("config.toml");
    let sqlite = codex_home.join("sqlite");
    fs::create_dir_all(&codex_home).unwrap();
    fs::write(&config, "model = \"gpt-native\"\n").unwrap();

    install_isolated_sqlite_home(&config, &codex_home, &sqlite).unwrap();
    let rendered = fs::read_to_string(config).unwrap();
    assert!(rendered.contains("model = \"gpt-native\""));
    assert!(rendered.contains("sqlite_home"));
    assert!(rendered.contains(sqlite.to_str().unwrap()));

    assert!(
        install_isolated_sqlite_home(
            &codex_home.join("config.toml"),
            &codex_home,
            &temp.path().join("outside")
        )
        .is_err()
    );
}
