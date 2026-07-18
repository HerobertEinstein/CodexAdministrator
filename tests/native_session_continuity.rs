use std::{collections::BTreeMap, env, fs, path::PathBuf};

use anyhow::Result;
use codex_administrator::{
    NativeSessionHead, NativeSessionHeadStore, NativeSessionRelation, NativeTurnCheckpoint,
    NativeTurnStatus, compare_native_session_heads, observe_native_session_continuity,
    observe_native_session_continuity_via_official_app_server,
};
use tempfile::tempdir;

fn turn(id: &str, fingerprint: &str, status: NativeTurnStatus) -> NativeTurnCheckpoint {
    NativeTurnCheckpoint {
        id: id.to_owned(),
        fingerprint: fingerprint.to_owned(),
        status,
    }
}

fn head(turns: Vec<NativeTurnCheckpoint>) -> NativeSessionHead {
    NativeSessionHead {
        thread_id: "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e".to_owned(),
        model_provider: "test".to_owned(),
        turns,
        history_complete: true,
    }
}

#[test]
fn equal_heads_share_the_same_latest_exact_turn() {
    let common = turn("turn-2", "sha-turn-2", NativeTurnStatus::Completed);
    let daily = head(vec![
        common.clone(),
        turn("turn-1", "sha-turn-1", NativeTurnStatus::Completed),
    ]);
    let isolated = head(vec![
        common,
        turn("turn-1", "sha-turn-1", NativeTurnStatus::Completed),
    ]);

    let continuity = compare_native_session_heads(&daily, &isolated).unwrap();

    assert_eq!(continuity.relation, NativeSessionRelation::Equal);
    assert_eq!(continuity.common_turn_id.as_deref(), Some("turn-2"));
    assert_eq!(continuity.daily_head_id.as_deref(), Some("turn-2"));
    assert_eq!(continuity.isolated_head_id.as_deref(), Some("turn-2"));
}

#[test]
fn a_lane_is_ahead_only_when_the_other_exact_head_is_in_its_history() {
    let daily = head(vec![
        turn("turn-3", "sha-turn-3", NativeTurnStatus::Completed),
        turn("turn-2", "sha-turn-2", NativeTurnStatus::Completed),
        turn("turn-1", "sha-turn-1", NativeTurnStatus::Completed),
    ]);
    let isolated = head(vec![
        turn("turn-2", "sha-turn-2", NativeTurnStatus::Completed),
        turn("turn-1", "sha-turn-1", NativeTurnStatus::Completed),
    ]);

    let continuity = compare_native_session_heads(&daily, &isolated).unwrap();

    assert_eq!(continuity.relation, NativeSessionRelation::DailyAhead);
    assert_eq!(continuity.common_turn_id.as_deref(), Some("turn-2"));
    assert_eq!(continuity.daily_head_id.as_deref(), Some("turn-3"));
    assert_eq!(continuity.isolated_head_id.as_deref(), Some("turn-2"));
}

#[test]
fn the_live_partial_copy_scenario_is_divergence_not_daily_ahead() {
    let daily = head(vec![
        turn(
            "019f729d-9374-7531-b112-07bd21fa432d",
            "daily-newest",
            NativeTurnStatus::Interrupted,
        ),
        turn(
            "019f716b-6b8c-7d20-9ac7-e12acbb07b8e",
            "daily-completed-version",
            NativeTurnStatus::Completed,
        ),
        turn(
            "019f7107-fa91-7a81-b012-edf375def599",
            "exact-common",
            NativeTurnStatus::Completed,
        ),
    ]);
    let isolated = head(vec![
        turn(
            "019f716b-6b8c-7d20-9ac7-e12acbb07b8e",
            "isolated-interrupted-version",
            NativeTurnStatus::Interrupted,
        ),
        turn(
            "019f7107-fa91-7a81-b012-edf375def599",
            "exact-common",
            NativeTurnStatus::Completed,
        ),
    ]);

    let continuity = compare_native_session_heads(&daily, &isolated).unwrap();

    assert_eq!(continuity.relation, NativeSessionRelation::Diverged);
    assert_eq!(
        continuity.common_turn_id.as_deref(),
        Some("019f7107-fa91-7a81-b012-edf375def599")
    );
    assert_eq!(
        continuity.daily_head_id.as_deref(),
        Some("019f729d-9374-7531-b112-07bd21fa432d")
    );
    assert_eq!(
        continuity.isolated_head_id.as_deref(),
        Some("019f716b-6b8c-7d20-9ac7-e12acbb07b8e")
    );
}

#[test]
fn different_thread_ids_are_rejected_instead_of_being_compared() {
    let daily = head(vec![]);
    let mut isolated = head(vec![]);
    isolated.thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26f".to_owned();

    let error = compare_native_session_heads(&daily, &isolated).unwrap_err();

    assert!(error.to_string().contains("same logical thread"));
}

#[test]
fn bounded_windows_without_a_common_turn_remain_unknown() {
    let mut daily = head(vec![turn(
        "turn-daily",
        "sha-daily",
        NativeTurnStatus::Completed,
    )]);
    daily.history_complete = false;
    let isolated = head(vec![turn(
        "turn-isolated",
        "sha-isolated",
        NativeTurnStatus::Completed,
    )]);

    let continuity = compare_native_session_heads(&daily, &isolated).unwrap();

    assert_eq!(continuity.relation, NativeSessionRelation::Unknown);
    assert_eq!(continuity.common_turn_id, None);
}

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
fn continuity_observation_persists_both_heads_and_the_exact_common_turn() {
    let thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e";
    let common = turn("turn-common", "sha-common", NativeTurnStatus::Completed);
    let mut daily = FakeHeadStore::default();
    daily.heads.insert(
        thread_id.to_owned(),
        head(vec![
            turn("turn-daily", "sha-daily", NativeTurnStatus::Completed),
            common.clone(),
        ]),
    );
    let mut isolated = FakeHeadStore::default();
    isolated.heads.insert(
        thread_id.to_owned(),
        head(vec![
            turn(
                "turn-isolated",
                "sha-isolated",
                NativeTurnStatus::Interrupted,
            ),
            common,
        ]),
    );
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("session-continuity-manifest.json");

    let receipt =
        observe_native_session_continuity(&mut daily, &mut isolated, [thread_id], &manifest)
            .unwrap();

    assert_eq!(receipt.threads, 1);
    assert_eq!(receipt.diverged, 1);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let record = &saved["records"][thread_id];
    assert_eq!(record["daily"]["turns"][0]["id"], "turn-daily");
    assert_eq!(record["isolated"]["turns"][0]["id"], "turn-isolated");
    assert_eq!(record["continuity"]["relation"], "diverged");
    assert_eq!(record["continuity"]["commonTurnId"], "turn-common");
}

#[test]
fn a_later_observation_replaces_only_the_small_head_record() {
    let thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e";
    let latest = turn("turn-latest", "sha-latest", NativeTurnStatus::Completed);
    let mut daily = FakeHeadStore::default();
    daily
        .heads
        .insert(thread_id.to_owned(), head(vec![latest.clone()]));
    let mut isolated = FakeHeadStore::default();
    isolated.heads.insert(
        thread_id.to_owned(),
        head(vec![turn(
            "turn-old",
            "sha-old",
            NativeTurnStatus::Completed,
        )]),
    );
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("session-continuity-manifest.json");
    observe_native_session_continuity(&mut daily, &mut isolated, [thread_id], &manifest).unwrap();

    isolated
        .heads
        .insert(thread_id.to_owned(), head(vec![latest]));
    let receipt =
        observe_native_session_continuity(&mut daily, &mut isolated, [thread_id], &manifest)
            .unwrap();

    assert_eq!(receipt.equal, 1);
    assert_eq!(receipt.diverged, 0);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    assert_eq!(
        saved["records"][thread_id]["continuity"]["relation"],
        "equal"
    );
}

#[test]
#[ignore = "requires two live Codex homes and an official npm Codex app-server"]
fn live_official_app_servers_report_the_two_real_session_heads() {
    let daily = PathBuf::from(env::var_os("CODEX_ADMINISTRATOR_TEST_DAILY_HOME").unwrap());
    let isolated = PathBuf::from(env::var_os("CODEX_ADMINISTRATOR_TEST_ISOLATED_HOME").unwrap());
    let thread_id = env::var("CODEX_ADMINISTRATOR_TEST_THREAD_ID").unwrap();
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("session-continuity-manifest.json");

    let receipt = observe_native_session_continuity_via_official_app_server(
        &daily,
        &isolated,
        [&thread_id],
        &manifest,
    )
    .unwrap()
    .expect("the official npm Codex app-server should be discoverable");

    assert_eq!(receipt.threads, 1);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    let record = &saved["records"][&thread_id];
    assert!(record["daily"]["turns"][0]["id"].is_string());
    assert!(record["isolated"]["turns"][0]["id"].is_string());
    assert!(record["continuity"]["relation"].is_string());
}
