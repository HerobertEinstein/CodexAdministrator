use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::os::windows::process::CommandExt;

use crate::{environment_variable_is_sensitive, install_bootstrap_atomically};

const MANIFEST_VERSION: u8 = 1;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PROTOCOL_LINE_BYTES: usize = 8 * 1024 * 1024;
const MAX_OBJECTIVE_BYTES: usize = 1024 * 1024;
const MAX_SHARED_THREADS: usize = 4096;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const APP_SERVER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeGoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeGoalIntent {
    pub objective: String,
    pub status: NativeGoalStatus,
    pub token_budget: Option<i64>,
}

impl NativeGoalIntent {
    fn validate(&self) -> Result<()> {
        if self.objective.trim().is_empty()
            || self.objective.len() > MAX_OBJECTIVE_BYTES
            || self.objective.contains('\0')
        {
            bail!("native Goal objective has an invalid shape or size");
        }
        if self.token_budget.is_some_and(|budget| budget <= 0) {
            bail!("native Goal token budget must be positive when present");
        }
        Ok(())
    }
}

pub trait NativeGoalStore {
    fn get_goal(&mut self, thread_id: &str) -> Result<Option<NativeGoalIntent>>;
    fn set_goal(&mut self, thread_id: &str, goal: &NativeGoalIntent) -> Result<()>;
    fn clear_goal(&mut self, thread_id: &str) -> Result<()>;
}

#[derive(Debug)]
struct JsonLineAppServer<R, W> {
    reader: R,
    writer: W,
    next_request_id: u64,
}

impl<R: BufRead, W: Write> JsonLineAppServer<R, W> {
    fn initialize(reader: R, writer: W, expected_codex_home: &Path) -> Result<Self> {
        let mut server = Self {
            reader,
            writer,
            next_request_id: 1,
        };
        let initialized = server.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "codex-administrator-goal-sync",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true,
                    "optOutNotificationMethods": []
                }
            }),
        )?;
        let actual_home = initialized
            .get("codexHome")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("official app-server initialize omitted CODEX_HOME"))?;
        let user_agent = initialized
            .get("userAgent")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !user_agent.starts_with("Codex Desktop/") {
            bail!("Goal sync helper did not identify as the official Codex app-server");
        }
        if canonical_existing_path(&actual_home)? != canonical_existing_path(expected_codex_home)? {
            bail!("official app-server initialized against a different CODEX_HOME");
        }
        server.notify("initialized", None)?;
        Ok(server)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let request_id = self.next_request_id.to_string();
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("official app-server request id overflow"))?;
        self.write_message(&json!({
            "id": request_id,
            "method": method,
            "params": params
        }))?;

        loop {
            let message = self.read_message()?;
            if message.get("id").and_then(Value::as_str) != Some(request_id.as_str()) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let code = error
                    .get("code")
                    .and_then(Value::as_i64)
                    .unwrap_or_default();
                let description = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown app-server error");
                bail!("official app-server request {method} failed ({code}): {description}");
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("official app-server response omitted result"));
        }
    }

    fn notify(&mut self, method: &str, params: Option<Value>) -> Result<()> {
        let mut message = json!({"method": method});
        if let Some(params) = params {
            message["params"] = params;
        }
        self.write_message(&message)
    }

    fn write_message(&mut self, message: &Value) -> Result<()> {
        serde_json::to_writer(&mut self.writer, message)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    fn read_message(&mut self) -> Result<Value> {
        let mut line = Vec::new();
        loop {
            let available = self.reader.fill_buf()?;
            if available.is_empty() {
                if line.is_empty() {
                    bail!("official app-server closed its response stream");
                }
                break;
            }
            let take = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            line.extend_from_slice(&available[..take]);
            self.reader.consume(take);
            if line.len() > MAX_PROTOCOL_LINE_BYTES {
                bail!("official app-server response line exceeds its size limit");
            }
            if line.last() == Some(&b'\n') {
                break;
            }
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        serde_json::from_slice(&line).context("official app-server emitted invalid JSON")
    }

    #[cfg(test)]
    fn into_writer(self) -> W {
        self.writer
    }
}

#[derive(Debug, Clone)]
struct AppServerCommand {
    program: PathBuf,
    prefix_args: Vec<OsString>,
}

impl AppServerCommand {
    fn spawn(&self, codex_home: &Path) -> Result<SpawnedGoalStore> {
        if !self.program.is_absolute() || !self.program.is_file() {
            bail!("official Codex app-server program must be an absolute regular file");
        }
        let mut command = Command::new(&self.program);
        command
            .args(&self.prefix_args)
            .args([
                "app-server",
                "--stdio",
                "--disable",
                "plugins",
                "--disable",
                "apps",
            ])
            .current_dir(codex_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        for (key, _) in env::vars_os() {
            if environment_variable_is_sensitive(&key) {
                command.env_remove(key);
            }
        }
        command.env("CODEX_HOME", codex_home);
        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to start official Codex app-server helper {}",
                self.program.display()
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("official app-server stdin was unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("official app-server stdout was unavailable"))?;
        let server = match JsonLineAppServer::initialize(
            BufReader::new(stdout),
            BufWriter::new(stdin),
            codex_home,
        ) {
            Ok(server) => server,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(SpawnedGoalStore {
            server: Some(server),
            child,
        })
    }
}

fn discover_codex_app_server_command() -> Result<Option<AppServerCommand>> {
    if let Some(program) = env::var_os("CODEX_ADMINISTRATOR_CODEX_APP_SERVER") {
        let program = canonical_regular_program(Path::new(&program))?;
        return Ok(Some(AppServerCommand {
            program,
            prefix_args: Vec::new(),
        }));
    }
    let path = env::var_os("PATH").unwrap_or_default();
    discover_codex_app_server_command_from_path(&path)
}

fn discover_codex_app_server_command_from_path(path: &OsStr) -> Result<Option<AppServerCommand>> {
    let mut entries = 0_usize;
    for directory in env::split_paths(path) {
        entries += 1;
        if entries > 512 {
            bail!("PATH contains too many entries for bounded Codex discovery");
        }
        let candidates = [
            directory
                .join("node_modules")
                .join("@openai")
                .join("codex")
                .join("node_modules")
                .join("@openai")
                .join("codex-win32-x64")
                .join("vendor")
                .join("x86_64-pc-windows-msvc")
                .join("bin")
                .join("codex.exe"),
            directory
                .join("node_modules")
                .join("@openai")
                .join("codex-win32-x64")
                .join("vendor")
                .join("x86_64-pc-windows-msvc")
                .join("bin")
                .join("codex.exe"),
        ];
        for candidate in candidates {
            if path_is_inside_windowsapps(&candidate) {
                continue;
            }
            let Some(program) = canonical_regular_program_if_present(&candidate)? else {
                continue;
            };
            return Ok(Some(AppServerCommand {
                program,
                prefix_args: Vec::new(),
            }));
        }
    }
    Ok(None)
}

fn canonical_regular_program(path: &Path) -> Result<PathBuf> {
    canonical_regular_program_if_present(path)?.ok_or_else(|| {
        anyhow::anyhow!(
            "configured official Codex app-server program does not exist: {}",
            path.display()
        )
    })
}

fn canonical_regular_program_if_present(path: &Path) -> Result<Option<PathBuf>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect Codex program {}", path.display()));
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        bail!("Codex app-server program must be a regular non-link file");
    }
    Ok(Some(fs::canonicalize(path).with_context(|| {
        format!("failed to resolve Codex program {}", path.display())
    })?))
}

fn path_is_inside_windowsapps(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|value| value.eq_ignore_ascii_case("WindowsApps"))
    })
}

struct SpawnedGoalStore {
    server: Option<JsonLineAppServer<BufReader<ChildStdout>, BufWriter<ChildStdin>>>,
    child: Child,
}

impl NativeGoalStore for SpawnedGoalStore {
    fn get_goal(&mut self, thread_id: &str) -> Result<Option<NativeGoalIntent>> {
        self.server_mut()?.get_goal(thread_id)
    }

    fn set_goal(&mut self, thread_id: &str, goal: &NativeGoalIntent) -> Result<()> {
        self.server_mut()?.set_goal(thread_id, goal)
    }

    fn clear_goal(&mut self, thread_id: &str) -> Result<()> {
        self.server_mut()?.clear_goal(thread_id)
    }
}

impl SpawnedGoalStore {
    fn server_mut(
        &mut self,
    ) -> Result<&mut JsonLineAppServer<BufReader<ChildStdout>, BufWriter<ChildStdin>>> {
        self.server
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("official app-server helper is already closed"))
    }

    fn shutdown(&mut self) {
        self.server.take();
        let deadline = Instant::now() + APP_SERVER_SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SpawnedGoalStore {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn sync_native_goal_intents_with_command<T>(
    command: &AppServerCommand,
    daily_codex_home: &Path,
    isolated_codex_home: &Path,
    thread_ids: T,
    manifest_path: &Path,
) -> Result<NativeGoalSyncReceipt>
where
    T: IntoIterator,
    T::Item: AsRef<str>,
{
    let daily = canonical_existing_path(daily_codex_home)?;
    let isolated = canonical_existing_path(isolated_codex_home)?;
    if daily == isolated {
        bail!("daily and isolated CODEX_HOME paths must be disjoint for Goal sync");
    }
    let mut daily_store = command.spawn(&daily)?;
    let mut isolated_store = command.spawn(&isolated)?;
    sync_native_goal_intents(
        &mut daily_store,
        &mut isolated_store,
        thread_ids,
        manifest_path,
    )
}

pub fn sync_native_goal_intents_via_official_app_server<T>(
    daily_codex_home: &Path,
    isolated_codex_home: &Path,
    thread_ids: T,
    manifest_path: &Path,
) -> Result<Option<NativeGoalSyncReceipt>>
where
    T: IntoIterator,
    T::Item: AsRef<str>,
{
    let Some(command) = discover_codex_app_server_command()? else {
        return Ok(None);
    };
    sync_native_goal_intents_with_command(
        &command,
        daily_codex_home,
        isolated_codex_home,
        thread_ids,
        manifest_path,
    )
    .map(Some)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppServerGoal {
    objective: String,
    status: NativeGoalStatus,
    token_budget: Option<i64>,
}

impl From<AppServerGoal> for NativeGoalIntent {
    fn from(goal: AppServerGoal) -> Self {
        Self {
            objective: goal.objective,
            status: goal.status,
            token_budget: goal.token_budget,
        }
    }
}

impl<R: BufRead, W: Write> NativeGoalStore for JsonLineAppServer<R, W> {
    fn get_goal(&mut self, thread_id: &str) -> Result<Option<NativeGoalIntent>> {
        let result = self.request("thread/goal/get", json!({"threadId": thread_id}))?;
        let goal = result.get("goal").cloned().unwrap_or(Value::Null);
        if goal.is_null() {
            return Ok(None);
        }
        Ok(Some(
            serde_json::from_value::<AppServerGoal>(goal)
                .context("official app-server returned an invalid Goal")?
                .into(),
        ))
    }

    fn set_goal(&mut self, thread_id: &str, goal: &NativeGoalIntent) -> Result<()> {
        goal.validate()?;
        let result = self.request(
            "thread/goal/set",
            json!({
                "threadId": thread_id,
                "objective": goal.objective,
                "status": goal.status,
                "tokenBudget": goal.token_budget
            }),
        )?;
        let returned = result
            .get("goal")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("official app-server Goal set omitted Goal"))?;
        let returned: NativeGoalIntent = serde_json::from_value::<AppServerGoal>(returned)
            .context("official app-server returned an invalid Goal after set")?
            .into();
        if &returned != goal {
            bail!("official app-server did not preserve the requested Goal intent");
        }
        Ok(())
    }

    fn clear_goal(&mut self, thread_id: &str) -> Result<()> {
        let result = self.request("thread/goal/clear", json!({"threadId": thread_id}))?;
        if result.get("cleared").and_then(Value::as_bool) != Some(true) {
            bail!("official app-server did not confirm Goal clear");
        }
        Ok(())
    }
}

fn canonical_existing_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path)
        .with_context(|| format!("failed to resolve CODEX_HOME {}", path.display()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeGoalSyncReceipt {
    pub threads: usize,
    pub unchanged: usize,
    pub copied_to_daily: usize,
    pub copied_to_isolated: usize,
    pub cleared_daily: usize,
    pub cleared_isolated: usize,
    pub conflicts: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct GoalIntentSyncManifest {
    version: u8,
    records: BTreeMap<String, GoalIntentSyncRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct GoalIntentSyncRecord {
    base: Option<NativeGoalIntent>,
    conflict: Option<GoalIntentConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GoalIntentConflict {
    daily: Option<NativeGoalIntent>,
    isolated: Option<NativeGoalIntent>,
}

pub fn sync_native_goal_intents<D, I, T>(
    daily: &mut D,
    isolated: &mut I,
    thread_ids: T,
    manifest_path: &Path,
) -> Result<NativeGoalSyncReceipt>
where
    D: NativeGoalStore,
    I: NativeGoalStore,
    T: IntoIterator,
    T::Item: AsRef<str>,
{
    if !manifest_path.is_absolute() {
        bail!("native Goal sync manifest path must be absolute");
    }
    let thread_ids = validated_thread_ids(thread_ids)?;
    let mut manifest = load_manifest(manifest_path)?;
    let mut receipt = NativeGoalSyncReceipt {
        threads: thread_ids.len(),
        ..NativeGoalSyncReceipt::default()
    };

    for thread_id in thread_ids {
        let daily_goal = validated_goal(daily.get_goal(&thread_id)?)?;
        let isolated_goal = validated_goal(isolated.get_goal(&thread_id)?)?;
        let prior = manifest.records.get(&thread_id).cloned();

        if daily_goal == isolated_goal {
            receipt.unchanged += 1;
            manifest.records.insert(
                thread_id,
                GoalIntentSyncRecord {
                    base: daily_goal,
                    conflict: None,
                },
            );
            continue;
        }

        match prior {
            Some(prior) if daily_goal == prior.base && isolated_goal != prior.base => {
                let base = prior.base;
                match apply_goal_if_unchanged(
                    daily,
                    &thread_id,
                    &daily_goal,
                    isolated_goal.as_ref(),
                    &mut receipt.copied_to_daily,
                    &mut receipt.cleared_daily,
                )? {
                    ApplyGoalResult::Applied => {
                        manifest.records.insert(
                            thread_id,
                            GoalIntentSyncRecord {
                                base: isolated_goal,
                                conflict: None,
                            },
                        );
                    }
                    ApplyGoalResult::Changed(current_daily) => record_conflict(
                        &mut manifest,
                        &mut receipt,
                        thread_id,
                        base,
                        current_daily,
                        isolated_goal,
                    ),
                }
            }
            Some(prior) if isolated_goal == prior.base && daily_goal != prior.base => {
                let base = prior.base;
                match apply_goal_if_unchanged(
                    isolated,
                    &thread_id,
                    &isolated_goal,
                    daily_goal.as_ref(),
                    &mut receipt.copied_to_isolated,
                    &mut receipt.cleared_isolated,
                )? {
                    ApplyGoalResult::Applied => {
                        manifest.records.insert(
                            thread_id,
                            GoalIntentSyncRecord {
                                base: daily_goal,
                                conflict: None,
                            },
                        );
                    }
                    ApplyGoalResult::Changed(current_isolated) => record_conflict(
                        &mut manifest,
                        &mut receipt,
                        thread_id,
                        base,
                        daily_goal,
                        current_isolated,
                    ),
                }
            }
            None if daily_goal.is_some() && isolated_goal.is_none() => {
                match apply_goal_if_unchanged(
                    isolated,
                    &thread_id,
                    &isolated_goal,
                    daily_goal.as_ref(),
                    &mut receipt.copied_to_isolated,
                    &mut receipt.cleared_isolated,
                )? {
                    ApplyGoalResult::Applied => {
                        manifest.records.insert(
                            thread_id,
                            GoalIntentSyncRecord {
                                base: daily_goal,
                                conflict: None,
                            },
                        );
                    }
                    ApplyGoalResult::Changed(current_isolated) => record_conflict(
                        &mut manifest,
                        &mut receipt,
                        thread_id,
                        None,
                        daily_goal,
                        current_isolated,
                    ),
                }
            }
            None if daily_goal.is_none() && isolated_goal.is_some() => {
                match apply_goal_if_unchanged(
                    daily,
                    &thread_id,
                    &daily_goal,
                    isolated_goal.as_ref(),
                    &mut receipt.copied_to_daily,
                    &mut receipt.cleared_daily,
                )? {
                    ApplyGoalResult::Applied => {
                        manifest.records.insert(
                            thread_id,
                            GoalIntentSyncRecord {
                                base: isolated_goal,
                                conflict: None,
                            },
                        );
                    }
                    ApplyGoalResult::Changed(current_daily) => record_conflict(
                        &mut manifest,
                        &mut receipt,
                        thread_id,
                        None,
                        current_daily,
                        isolated_goal,
                    ),
                }
            }
            prior => {
                receipt.conflicts += 1;
                manifest.records.insert(
                    thread_id,
                    GoalIntentSyncRecord {
                        base: prior.and_then(|record| record.base),
                        conflict: Some(GoalIntentConflict {
                            daily: daily_goal,
                            isolated: isolated_goal,
                        }),
                    },
                );
            }
        }
    }

    manifest.version = MANIFEST_VERSION;
    persist_manifest(manifest_path, &manifest)?;
    Ok(receipt)
}

fn validated_thread_ids<T>(thread_ids: T) -> Result<Vec<String>>
where
    T: IntoIterator,
    T::Item: AsRef<str>,
{
    let mut unique = BTreeSet::new();
    for thread_id in thread_ids {
        let thread_id = thread_id.as_ref();
        if !valid_thread_id(thread_id) {
            bail!("native Goal sync received an invalid thread id");
        }
        unique.insert(thread_id.to_owned());
        if unique.len() > MAX_SHARED_THREADS {
            bail!("native Goal sync exceeds its shared-thread limit");
        }
    }
    Ok(unique.into_iter().collect())
}

fn valid_thread_id(thread_id: &str) -> bool {
    let bytes = thread_id.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}

fn validated_goal(goal: Option<NativeGoalIntent>) -> Result<Option<NativeGoalIntent>> {
    if let Some(goal) = &goal {
        goal.validate()?;
    }
    Ok(goal)
}

enum ApplyGoalResult {
    Applied,
    Changed(Option<NativeGoalIntent>),
}

fn apply_goal_if_unchanged<S: NativeGoalStore>(
    store: &mut S,
    thread_id: &str,
    expected: &Option<NativeGoalIntent>,
    goal: Option<&NativeGoalIntent>,
    copied: &mut usize,
    cleared: &mut usize,
) -> Result<ApplyGoalResult> {
    let current = validated_goal(store.get_goal(thread_id)?)?;
    if &current != expected {
        return Ok(ApplyGoalResult::Changed(current));
    }
    match goal {
        Some(goal) => {
            store.set_goal(thread_id, goal)?;
            *copied += 1;
        }
        None => {
            store.clear_goal(thread_id)?;
            *cleared += 1;
        }
    }
    Ok(ApplyGoalResult::Applied)
}

fn record_conflict(
    manifest: &mut GoalIntentSyncManifest,
    receipt: &mut NativeGoalSyncReceipt,
    thread_id: String,
    base: Option<NativeGoalIntent>,
    daily: Option<NativeGoalIntent>,
    isolated: Option<NativeGoalIntent>,
) {
    receipt.conflicts += 1;
    manifest.records.insert(
        thread_id,
        GoalIntentSyncRecord {
            base,
            conflict: Some(GoalIntentConflict { daily, isolated }),
        },
    );
}

fn load_manifest(path: &Path) -> Result<GoalIntentSyncManifest> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(GoalIntentSyncManifest::default());
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("failed to read native Goal manifest {}", path.display())
            });
        }
    };
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        bail!("native Goal sync manifest exceeds its size limit");
    }
    let manifest: GoalIntentSyncManifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse native Goal manifest {}", path.display()))?;
    if manifest.version != MANIFEST_VERSION {
        bail!("native Goal sync manifest version is unsupported");
    }
    if manifest.records.len() > MAX_SHARED_THREADS {
        bail!("native Goal sync manifest exceeds its shared-thread limit");
    }
    for (thread_id, record) in &manifest.records {
        if !valid_thread_id(thread_id) {
            bail!("native Goal sync manifest contains an invalid thread id");
        }
        validated_goal(record.base.clone())?;
        if let Some(conflict) = &record.conflict {
            validated_goal(conflict.daily.clone())?;
            validated_goal(conflict.isolated.clone())?;
        }
    }
    Ok(manifest)
}

fn persist_manifest(path: &Path, manifest: &GoalIntentSyncManifest) -> Result<()> {
    let content = serde_json::to_vec_pretty(manifest)?;
    if content.len() as u64 > MAX_MANIFEST_BYTES {
        bail!("native Goal sync manifest exceeds its size limit");
    }
    install_bootstrap_atomically(path, &content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{env, fs, io::Cursor, path::PathBuf};

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn json_line_app_server_uses_official_goal_methods_and_ignores_usage_counters() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("codex-home");
        fs::create_dir_all(&home).unwrap();
        let goal = NativeGoalIntent {
            objective: "Keep the official update boundary".into(),
            status: NativeGoalStatus::Active,
            token_budget: Some(75_000),
        };
        let responses = [
            json!({
                "id": "1",
                "result": {
                    "codexHome": home,
                    "platformFamily": "windows",
                    "platformOs": "windows",
                    "userAgent": "Codex Desktop/test"
                }
            }),
            json!({
                "method": "thread/goal/updated",
                "params": {"threadId": "ignored"}
            }),
            json!({
                "id": "2",
                "result": {
                    "goal": {
                        "threadId": "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e",
                        "objective": goal.objective,
                        "status": "active",
                        "tokenBudget": 75000,
                        "tokensUsed": 1234,
                        "timeUsedSeconds": 56,
                        "createdAt": 1,
                        "updatedAt": 2
                    }
                }
            }),
            json!({
                "id": "3",
                "result": {
                    "goal": {
                        "threadId": "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e",
                        "objective": goal.objective,
                        "status": "active",
                        "tokenBudget": 75000,
                        "tokensUsed": 0,
                        "timeUsedSeconds": 0,
                        "createdAt": 3,
                        "updatedAt": 3
                    }
                }
            }),
            json!({"id": "4", "result": {"cleared": true}}),
        ];
        let input = responses
            .into_iter()
            .map(|value| serde_json::to_string(&value).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let mut server =
            JsonLineAppServer::initialize(Cursor::new(input.into_bytes()), Vec::new(), &home)
                .unwrap();

        assert_eq!(
            server
                .get_goal("019f2164-bb7b-76a1-bed5-8f7ff7f6a26e")
                .unwrap(),
            Some(goal.clone())
        );
        server
            .set_goal("019f2164-bb7b-76a1-bed5-8f7ff7f6a26e", &goal)
            .unwrap();
        server
            .clear_goal("019f2164-bb7b-76a1-bed5-8f7ff7f6a26e")
            .unwrap();

        let output = String::from_utf8(server.into_writer()).unwrap();
        let requests = output
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(requests[0]["method"], "initialize");
        assert_eq!(requests[1]["method"], "initialized");
        assert_eq!(requests[2]["method"], "thread/goal/get");
        assert_eq!(requests[3]["method"], "thread/goal/set");
        assert_eq!(requests[3]["params"]["tokenBudget"], 75000);
        assert!(requests[3]["params"].get("tokensUsed").is_none());
        assert_eq!(requests[4]["method"], "thread/goal/clear");
    }

    #[test]
    fn json_line_app_server_rejects_a_different_codex_home() {
        let temp = tempdir().unwrap();
        let expected = temp.path().join("expected");
        let wrong = temp.path().join("wrong");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&wrong).unwrap();
        let input = serde_json::to_string(&json!({
            "id": "1",
            "result": {
                "codexHome": wrong,
                "platformFamily": "windows",
                "platformOs": "windows",
                "userAgent": "Codex Desktop/test"
            }
        }))
        .unwrap()
            + "\n";

        let error =
            JsonLineAppServer::initialize(Cursor::new(input.into_bytes()), Vec::new(), &expected)
                .unwrap_err();

        assert!(error.to_string().contains("different CODEX_HOME"));
    }

    #[test]
    fn spawned_app_servers_sync_goal_intent_between_disjoint_homes() {
        let temp = tempdir().unwrap();
        let daily = temp.path().join("daily");
        let isolated = temp.path().join("isolated");
        fs::create_dir_all(&daily).unwrap();
        fs::create_dir_all(&isolated).unwrap();
        let thread_id = "019f2164-bb7b-76a1-bed5-8f7ff7f6a26e";
        let initial = goal_fixture("Daily objective");
        fs::write(
            daily.join("fake-goals.json"),
            serde_json::to_vec(&json!({thread_id: initial})).unwrap(),
        )
        .unwrap();
        let script = temp.path().join("fake-app-server.mjs");
        fs::write(&script, FAKE_APP_SERVER).unwrap();
        let command = AppServerCommand {
            program: find_node_executable(),
            prefix_args: vec![script.into_os_string()],
        };
        let manifest = isolated.join("goal-intent-sync-manifest.json");

        let first = sync_native_goal_intents_with_command(
            &command,
            &daily,
            &isolated,
            [thread_id],
            &manifest,
        )
        .unwrap();

        assert_eq!(first.copied_to_isolated, 1);
        let isolated_goals: Value =
            serde_json::from_slice(&fs::read(isolated.join("fake-goals.json")).unwrap()).unwrap();
        assert_eq!(isolated_goals[thread_id]["objective"], "Daily objective");

        let isolated_change = goal_fixture("Isolated objective");
        fs::write(
            isolated.join("fake-goals.json"),
            serde_json::to_vec(&json!({thread_id: isolated_change})).unwrap(),
        )
        .unwrap();
        let second = sync_native_goal_intents_with_command(
            &command,
            &daily,
            &isolated,
            [thread_id],
            &manifest,
        )
        .unwrap();

        assert_eq!(second.copied_to_daily, 1);
        let daily_goals: Value =
            serde_json::from_slice(&fs::read(daily.join("fake-goals.json")).unwrap()).unwrap();
        assert_eq!(daily_goals[thread_id]["objective"], "Isolated objective");
    }

    #[test]
    fn npm_codex_native_app_server_is_discovered_without_using_the_windowsapps_alias() {
        let temp = tempdir().unwrap();
        let bin = temp.path().join("bin");
        let native = bin
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe");
        fs::create_dir_all(native.parent().unwrap()).unwrap();
        fs::write(&native, b"fixture").unwrap();
        let windows_apps = temp.path().join("WindowsApps");
        fs::create_dir_all(&windows_apps).unwrap();
        fs::write(windows_apps.join("codex.exe"), b"alias").unwrap();
        let path = env::join_paths([windows_apps, bin]).unwrap();

        let command = discover_codex_app_server_command_from_path(&path)
            .unwrap()
            .expect("the npm Codex native binary should be discovered");

        assert_eq!(command.program, fs::canonicalize(native).unwrap());
        assert!(command.prefix_args.is_empty());
    }

    fn goal_fixture(objective: &str) -> NativeGoalIntent {
        NativeGoalIntent {
            objective: objective.into(),
            status: NativeGoalStatus::Active,
            token_budget: Some(25_000),
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
import { readFileSync, writeFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { join } from "node:path";

const home = process.env.CODEX_HOME;
const statePath = join(home, "fake-goals.json");
const load = () => {
  try { return JSON.parse(readFileSync(statePath, "utf8")); }
  catch { return {}; }
};
const save = (state) => writeFileSync(statePath, JSON.stringify(state));
const send = (message) => process.stdout.write(JSON.stringify(message) + "\n");

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
  const state = load();
  const threadId = request.params.threadId;
  if (request.method === "thread/goal/get") {
    const goal = state[threadId] ?? null;
    send({ id: request.id, result: { goal: goal && {
      threadId,
      ...goal,
      tokensUsed: 99,
      timeUsedSeconds: 7,
      createdAt: 1,
      updatedAt: 2
    }}});
    return;
  }
  if (request.method === "thread/goal/set") {
    state[threadId] = {
      objective: request.params.objective,
      status: request.params.status,
      tokenBudget: request.params.tokenBudget
    };
    save(state);
    send({ id: request.id, result: { goal: {
      threadId,
      ...state[threadId],
      tokensUsed: 0,
      timeUsedSeconds: 0,
      createdAt: 3,
      updatedAt: 3
    }}});
    return;
  }
  if (request.method === "thread/goal/clear") {
    delete state[threadId];
    save(state);
    send({ id: request.id, result: { cleared: true }});
    return;
  }
  send({ id: request.id, error: { code: -32601, message: "unsupported" }});
});
"#;
}
