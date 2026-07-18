#![cfg(windows)]

use std::{
    env, fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use codex_administrator::{
    NativeSessionChangeMonitor, NativeSessionContinuityCoordinator,
    NativeSessionContinuityProcessBackend, NativeSessionContinuityWorker,
    NativeSessionContinuityWorkerBackend, NativeSessionContinuityWorkerOutcome,
    NativeSharedSessionRollout,
};
use tempfile::{TempDir, tempdir};

#[derive(Clone)]
struct WorkerControl {
    finish: Arc<AtomicBool>,
    terminated: Arc<AtomicBool>,
}

struct FakeWorker {
    control: WorkerControl,
}

impl NativeSessionContinuityWorker for FakeWorker {
    fn try_finish(&mut self) -> Result<Option<NativeSessionContinuityWorkerOutcome>> {
        Ok(self.control.finish.load(Ordering::SeqCst).then_some(
            NativeSessionContinuityWorkerOutcome {
                success: true,
                exit_code: Some(0),
                diagnostic: String::new(),
            },
        ))
    }

    fn terminate(&mut self) {
        self.control.terminated.store(true, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct FakeBackendState {
    batches: Vec<Vec<String>>,
    controls: Vec<WorkerControl>,
}

struct FakeBackend {
    state: Arc<Mutex<FakeBackendState>>,
}

impl NativeSessionContinuityWorkerBackend for FakeBackend {
    fn start(&mut self, thread_ids: &[String]) -> Result<Box<dyn NativeSessionContinuityWorker>> {
        let control = WorkerControl {
            finish: Arc::new(AtomicBool::new(false)),
            terminated: Arc::new(AtomicBool::new(false)),
        };
        let mut state = self.state.lock().unwrap();
        state.batches.push(thread_ids.to_vec());
        state.controls.push(control.clone());
        Ok(Box::new(FakeWorker { control }))
    }
}

#[test]
fn changes_during_an_active_worker_are_coalesced_into_one_follow_up_batch() {
    let fixture = fixture();
    let state = Arc::new(Mutex::new(FakeBackendState::default()));
    let backend = FakeBackend {
        state: Arc::clone(&state),
    };
    let mut coordinator =
        NativeSessionContinuityCoordinator::new(fixture.monitor, Box::new(backend));

    fs::write(&fixture.daily_rollout, "daily first change\n").unwrap();
    wait_until(Duration::from_secs(3), || {
        coordinator.maintain_once().unwrap();
        state.lock().unwrap().batches.len() == 1
    });

    fs::write(&fixture.daily_rollout, "daily second change\n").unwrap();
    thread::sleep(Duration::from_millis(20));
    fs::write(&fixture.daily_rollout, "daily third change\n").unwrap();
    wait_until(Duration::from_secs(3), || {
        let receipt = coordinator.maintain_once().unwrap();
        !receipt.changed_threads.is_empty()
    });
    assert_eq!(state.lock().unwrap().batches.len(), 1);

    state.lock().unwrap().controls[0]
        .finish
        .store(true, Ordering::SeqCst);
    wait_until(Duration::from_secs(3), || {
        coordinator.maintain_once().unwrap();
        state.lock().unwrap().batches.len() == 2
    });

    let batches = &state.lock().unwrap().batches;
    assert_eq!(batches[0], vec![fixture.thread_id.clone()]);
    assert_eq!(batches[1], vec![fixture.thread_id]);
}

#[test]
fn dropping_the_coordinator_terminates_an_active_worker() {
    let fixture = fixture();
    let state = Arc::new(Mutex::new(FakeBackendState::default()));
    let backend = FakeBackend {
        state: Arc::clone(&state),
    };
    let mut coordinator =
        NativeSessionContinuityCoordinator::new(fixture.monitor, Box::new(backend));

    fs::write(&fixture.daily_rollout, "daily changed\n").unwrap();
    wait_until(Duration::from_secs(3), || {
        coordinator.maintain_once().unwrap();
        !state.lock().unwrap().controls.is_empty()
    });
    let terminated = Arc::clone(&state.lock().unwrap().controls[0].terminated);

    drop(coordinator);

    assert!(terminated.load(Ordering::SeqCst));
}

#[test]
fn explicit_seed_starts_a_worker_without_waiting_for_a_rollout_change() {
    let fixture = fixture();
    let state = Arc::new(Mutex::new(FakeBackendState::default()));
    let backend = FakeBackend {
        state: Arc::clone(&state),
    };
    let mut coordinator =
        NativeSessionContinuityCoordinator::new(fixture.monitor, Box::new(backend));

    coordinator
        .enqueue_threads([fixture.thread_id.clone()])
        .unwrap();
    let receipt = coordinator.maintain_once().unwrap();

    assert_eq!(receipt.changed_threads, Vec::<String>::new());
    assert_eq!(receipt.started_threads, vec![fixture.thread_id.clone()]);
    assert_eq!(state.lock().unwrap().batches[0], vec![fixture.thread_id]);
}

#[test]
fn process_worker_runs_out_of_band_and_persists_exact_dual_head_cursors() {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    fs::create_dir_all(&daily).unwrap();
    fs::create_dir_all(&isolated).unwrap();
    let thread_id = "019f0000-0000-7000-8000-000000000002";
    for home in [&daily, &isolated] {
        fs::write(home.join("app-server"), FAKE_APP_SERVER).unwrap();
    }
    fs::write(
        daily.join("fake-turns.json"),
        serde_json::to_vec(&serde_json::json!({thread_id: [{
            "id": "turn-daily",
            "items": [{"id": "item-daily-summary", "type": "agentMessage"}],
            "status": "inProgress",
            "error": null,
            "startedAt": 1,
            "completedAt": null,
            "durationMs": null
        }]}))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        isolated.join("fake-turns.json"),
        serde_json::to_vec(&serde_json::json!({thread_id: [{
            "id": "turn-isolated",
            "items": [{"id": "item-isolated-summary", "type": "agentMessage"}],
            "status": "interrupted",
            "error": null,
            "startedAt": 1,
            "completedAt": null,
            "durationMs": null
        }]}))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        daily.join("fake-turn-items.json"),
        serde_json::to_vec(&serde_json::json!({thread_id: {"turn-daily": [
            {"id": "item-daily-summary", "type": "agentMessage", "text": "private"},
            {"id": "item-daily-tool", "type": "dynamicToolCall", "status": "inProgress"}
        ]}}))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        isolated.join("fake-turn-items.json"),
        serde_json::to_vec(&serde_json::json!({thread_id: {"turn-isolated": [
            {"id": "item-isolated-final", "type": "agentMessage", "text": "private"}
        ]}}))
        .unwrap(),
    )
    .unwrap();
    let request = isolated.join("session-continuity-worker-request.json");
    let manifest = isolated.join("session-continuity-manifest.json");
    let mut backend = NativeSessionContinuityProcessBackend::new(
        PathBuf::from(env!("CARGO_BIN_EXE_codex-administrator")),
        daily,
        isolated,
        request,
        manifest.clone(),
    )
    .unwrap()
    .with_app_server_program(find_node_executable())
    .unwrap();

    let mut worker = backend.start(&[thread_id.to_owned()]).unwrap();
    let outcome = wait_for_worker(&mut *worker);

    assert!(outcome.success, "{}", outcome.diagnostic);
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    assert_eq!(
        saved["records"][thread_id]["continuity"]["dailyCursor"]["itemId"],
        "item-daily-tool"
    );
    assert_eq!(
        saved["records"][thread_id]["continuity"]["isolatedCursor"]["itemId"],
        "item-isolated-final"
    );
    assert!(!saved.to_string().contains("private"));
}

struct Fixture {
    _temp: TempDir,
    monitor: NativeSessionChangeMonitor,
    daily_rollout: std::path::PathBuf,
    thread_id: String,
}

fn fixture() -> Fixture {
    let temp = tempdir().unwrap();
    let daily = temp.path().join("daily");
    let isolated = temp.path().join("isolated");
    let thread_id = "019f0000-0000-7000-8000-000000000001".to_owned();
    let relative = format!("sessions/2026/07/18/rollout-2026-07-18T00-00-00-{thread_id}.jsonl");
    let daily_rollout = daily.join(&relative);
    let isolated_rollout = isolated.join(relative);
    fs::create_dir_all(daily_rollout.parent().unwrap()).unwrap();
    fs::create_dir_all(isolated_rollout.parent().unwrap()).unwrap();
    fs::write(&daily_rollout, "daily\n").unwrap();
    fs::write(&isolated_rollout, "isolated\n").unwrap();
    let rollout = NativeSharedSessionRollout {
        thread_id: thread_id.clone(),
        daily_path: fs::canonicalize(&daily_rollout).unwrap(),
        isolated_path: fs::canonicalize(&isolated_rollout).unwrap(),
    };
    let monitor =
        NativeSessionChangeMonitor::new(&daily, &isolated, [rollout], Duration::from_millis(60))
            .unwrap();
    Fixture {
        _temp: temp,
        monitor,
        daily_rollout,
        thread_id,
    }
}

fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !predicate() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_worker(
    worker: &mut dyn NativeSessionContinuityWorker,
) -> NativeSessionContinuityWorkerOutcome {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(outcome) = worker.try_finish().unwrap() {
            return outcome;
        }
        assert!(Instant::now() < deadline, "worker timed out");
        thread::sleep(Duration::from_millis(20));
    }
}

fn find_node_executable() -> PathBuf {
    if let Some(node) = env::var_os("NODE") {
        let node = PathBuf::from(node);
        if node.is_file() {
            return node;
        }
    }
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|directory| directory.join("node.exe"))
        .find(|candidate| candidate.is_file())
        .expect("Node.js is required by the repository test suite")
}

const FAKE_APP_SERVER: &str = r#"
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { join } from "node:path";

const home = process.env.CODEX_HOME;
const send = (message) => process.stdout.write(JSON.stringify(message) + "\n");
const load = (name) => {
  try { return JSON.parse(readFileSync(join(home, name), "utf8")); }
  catch { return {}; }
};

createInterface({ input: process.stdin, crlfDelay: Infinity }).on("line", (line) => {
  const request = JSON.parse(line);
  if (request.method === "initialized") return;
  if (request.method === "initialize") {
    send({ id: request.id, result: {
      codexHome: home,
      platformFamily: "windows",
      platformOs: "windows",
      userAgent: "Codex Desktop/fake"
    }});
    return;
  }
  const threadId = request.params.threadId;
  if (request.method === "thread/read") {
    send({ id: request.id, result: { thread: {
      id: threadId,
      sessionId: threadId,
      modelProvider: home.endsWith("isolated") ? "grok_native" : "openai",
      status: { type: "notLoaded" },
      turns: [],
      cliVersion: "fake",
      createdAt: 1,
      updatedAt: 2,
      cwd: home,
      ephemeral: false,
      preview: ""
    }}});
    return;
  }
  if (request.method === "thread/turns/list") {
    const allTurns = load("fake-turns.json")[threadId] ?? [];
    const start = Number(request.params.cursor ?? 0);
    const limit = Number(request.params.limit ?? allTurns.length);
    let data = allTurns.slice(start, start + limit);
    if (request.params.itemsView === "full") {
      const turnItems = load("fake-turn-items.json");
      data = data.map((turn) => ({
        ...turn,
        items: turnItems[threadId]?.[turn.id] ?? turn.items,
        itemsView: "full"
      }));
    }
    const next = start + data.length;
    send({ id: request.id, result: {
      data,
      nextCursor: next < allTurns.length ? String(next) : null,
      backwardsCursor: null
    }});
    return;
  }
  if (request.method === "thread/turns/items/list") {
    send({ id: request.id, error: {
      code: -32601,
      message: "thread/turns/items/list is not supported yet"
    }});
    return;
  }
  send({ id: request.id, error: { code: -32601, message: "unsupported" } });
});
"#;
