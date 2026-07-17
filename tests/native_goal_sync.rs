#![cfg(windows)]

use std::{collections::BTreeMap, path::Path};

use anyhow::Result;
use codex_administrator::{
    NativeGoalIntent, NativeGoalStatus, NativeGoalStore, sync_native_goal_intents,
};
use tempfile::tempdir;

#[derive(Default)]
struct MemoryGoalStore {
    goals: BTreeMap<String, NativeGoalIntent>,
}

struct RacingGoalStore {
    current: Option<NativeGoalIntent>,
    raced: NativeGoalIntent,
    reads: usize,
    writes: usize,
}

impl NativeGoalStore for RacingGoalStore {
    fn get_goal(&mut self, _thread_id: &str) -> Result<Option<NativeGoalIntent>> {
        self.reads += 1;
        if self.reads == 1 {
            return Ok(self.current.clone());
        }
        self.current = Some(self.raced.clone());
        Ok(self.current.clone())
    }

    fn set_goal(&mut self, _thread_id: &str, goal: &NativeGoalIntent) -> Result<()> {
        self.writes += 1;
        self.current = Some(goal.clone());
        Ok(())
    }

    fn clear_goal(&mut self, _thread_id: &str) -> Result<()> {
        self.writes += 1;
        self.current = None;
        Ok(())
    }
}

impl MemoryGoalStore {
    fn with_goal(mut self, thread_id: &str, goal: NativeGoalIntent) -> Self {
        self.goals.insert(thread_id.to_owned(), goal);
        self
    }
}

impl NativeGoalStore for MemoryGoalStore {
    fn get_goal(&mut self, thread_id: &str) -> Result<Option<NativeGoalIntent>> {
        Ok(self.goals.get(thread_id).cloned())
    }

    fn set_goal(&mut self, thread_id: &str, goal: &NativeGoalIntent) -> Result<()> {
        self.goals.insert(thread_id.to_owned(), goal.clone());
        Ok(())
    }

    fn clear_goal(&mut self, thread_id: &str) -> Result<()> {
        self.goals.remove(thread_id);
        Ok(())
    }
}

fn goal(objective: &str) -> NativeGoalIntent {
    NativeGoalIntent {
        objective: objective.to_owned(),
        status: NativeGoalStatus::Active,
        token_budget: Some(50_000),
    }
}

fn sync(
    daily: &mut MemoryGoalStore,
    isolated: &mut MemoryGoalStore,
    manifest: &Path,
) -> codex_administrator::NativeGoalSyncReceipt {
    sync_native_goal_intents(
        daily,
        isolated,
        ["019f2164-bb7b-76a1-bed5-8f7ff7f6a26e"],
        manifest,
    )
    .unwrap()
}

#[test]
fn goal_intent_changes_flow_in_both_directions_without_copying_usage_counters() {
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("goal-intent-sync-manifest.json");
    let thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e";
    let initial = goal("Keep the launcher isolated and update-safe");
    let mut daily = MemoryGoalStore::default().with_goal(thread_id, initial.clone());
    let mut isolated = MemoryGoalStore::default();

    let first = sync(&mut daily, &mut isolated, &manifest);
    assert_eq!(first.copied_to_isolated, 1);
    assert_eq!(isolated.goals.get(thread_id), Some(&initial));

    let isolated_change = goal("Preserve seamless task continuity");
    isolated
        .goals
        .insert(thread_id.to_owned(), isolated_change.clone());
    let second = sync(&mut daily, &mut isolated, &manifest);

    assert_eq!(second.copied_to_daily, 1);
    assert_eq!(daily.goals.get(thread_id), Some(&isolated_change));
}

#[test]
fn divergent_goal_changes_are_preserved_as_a_review_conflict() {
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("goal-intent-sync-manifest.json");
    let thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e";
    let baseline = goal("Baseline");
    let mut daily = MemoryGoalStore::default().with_goal(thread_id, baseline.clone());
    let mut isolated = MemoryGoalStore::default().with_goal(thread_id, baseline);
    let first = sync(&mut daily, &mut isolated, &manifest);
    assert_eq!(first.unchanged, 1);

    let daily_change = goal("Daily change");
    let isolated_change = goal("Isolated change");
    daily
        .goals
        .insert(thread_id.to_owned(), daily_change.clone());
    isolated
        .goals
        .insert(thread_id.to_owned(), isolated_change.clone());

    let second = sync(&mut daily, &mut isolated, &manifest);

    assert_eq!(second.conflicts, 1);
    assert_eq!(daily.goals.get(thread_id), Some(&daily_change));
    assert_eq!(isolated.goals.get(thread_id), Some(&isolated_change));
}

#[test]
fn a_one_sided_goal_clear_is_applied_through_the_store_api() {
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("goal-intent-sync-manifest.json");
    let thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e";
    let baseline = goal("Temporary objective");
    let mut daily = MemoryGoalStore::default().with_goal(thread_id, baseline.clone());
    let mut isolated = MemoryGoalStore::default().with_goal(thread_id, baseline);
    sync(&mut daily, &mut isolated, &manifest);

    daily.goals.remove(thread_id);
    let receipt = sync(&mut daily, &mut isolated, &manifest);

    assert_eq!(receipt.cleared_isolated, 1);
    assert!(!isolated.goals.contains_key(thread_id));
}

#[test]
fn a_concurrent_destination_change_becomes_a_conflict_instead_of_being_overwritten() {
    let temp = tempdir().unwrap();
    let manifest = temp.path().join("goal-intent-sync-manifest.json");
    let thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e";
    let daily_goal = goal("Daily objective");
    let raced_goal = goal("Concurrent isolated objective");
    let mut daily = MemoryGoalStore::default().with_goal(thread_id, daily_goal);
    let mut isolated = RacingGoalStore {
        current: None,
        raced: raced_goal.clone(),
        reads: 0,
        writes: 0,
    };

    let receipt =
        sync_native_goal_intents(&mut daily, &mut isolated, [thread_id], &manifest).unwrap();

    assert_eq!(receipt.conflicts, 1);
    assert_eq!(isolated.writes, 0);
    assert_eq!(isolated.current, Some(raced_goal));
}
