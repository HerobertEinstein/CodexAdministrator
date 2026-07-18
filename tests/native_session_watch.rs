#![cfg(windows)]

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use codex_administrator::{NativeSessionChangeMonitor, NativeSharedSessionRollout};
use tempfile::tempdir;

#[test]
fn rollout_change_notifications_emit_only_the_changed_shared_thread_after_quiet() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let thread_id = "019f0000-0000-7000-8000-000000000001";
    let relative = format!("sessions/2026/07/18/rollout-2026-07-18T00-00-00-{thread_id}.jsonl");
    let daily_rollout = daily.join(&relative);
    let isolated_rollout = isolated.join(&relative);
    fs::create_dir_all(daily_rollout.parent().unwrap()).unwrap();
    fs::create_dir_all(isolated_rollout.parent().unwrap()).unwrap();
    fs::write(&daily_rollout, "daily\n").unwrap();
    fs::write(&isolated_rollout, "isolated\n").unwrap();
    let tracked = NativeSharedSessionRollout {
        thread_id: thread_id.to_owned(),
        daily_path: fs::canonicalize(&daily_rollout).unwrap(),
        isolated_path: fs::canonicalize(&isolated_rollout).unwrap(),
    };
    let mut monitor =
        NativeSessionChangeMonitor::new(&daily, &isolated, [tracked], Duration::from_millis(60))
            .unwrap();

    fs::write(&daily_rollout, "daily changed\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let changed = loop {
        let changed = monitor.poll_changed().unwrap();
        if !changed.is_empty() {
            break changed;
        }
        assert!(Instant::now() < deadline, "change notification timed out");
        thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(changed, vec![thread_id]);
    assert!(monitor.poll_changed().unwrap().is_empty());
}
