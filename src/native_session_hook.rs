use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    NativeSessionContinuitySnapshot, NativeSessionCursor, NativeSessionRelation, NativeTurnStatus,
    install_bootstrap_atomically, read_native_session_continuity,
};

const MAX_HOOK_INPUT_BYTES: usize = 64 * 1024;
const MAX_HOOK_CONFIG_BYTES: u64 = 2 * 1024 * 1024;
const CONTINUITY_HOOK_MARKER: &str = "session-continuity-hook";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSessionLane {
    Daily,
    Isolated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSessionHookInstallReceipt {
    pub path: PathBuf,
    pub updated: bool,
}

#[derive(Debug, Deserialize)]
struct UserPromptSubmitHookInput {
    hook_event_name: String,
    session_id: String,
    turn_id: String,
}

pub fn native_session_continuity_hook_response(
    manifest_path: &Path,
    lane: NativeSessionLane,
    input: &[u8],
) -> Result<Value> {
    if input.len() > MAX_HOOK_INPUT_BYTES {
        bail!("session continuity hook input exceeds its size limit");
    }
    let input: UserPromptSubmitHookInput =
        serde_json::from_slice(input).context("session continuity hook input is invalid")?;
    if input.hook_event_name != "UserPromptSubmit" {
        bail!("session continuity hook received an unexpected event");
    }
    validate_thread_id(&input.session_id)?;
    validate_turn_id(&input.turn_id)?;
    let snapshot = match read_native_session_continuity(manifest_path, &input.session_id) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) | Err(_) => return Ok(noop_response()),
    };
    let context = render_context(&snapshot, lane, &input.turn_id);
    Ok(json!({
        "continue": true,
        "suppressOutput": true,
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": context
        }
    }))
}

pub fn install_native_session_continuity_hook_file(
    codex_home: &Path,
    command: &str,
) -> Result<NativeSessionHookInstallReceipt> {
    if !codex_home.is_absolute() {
        bail!("session continuity hook CODEX_HOME must be absolute");
    }
    if command.trim().is_empty()
        || command.len() > 8192
        || command.chars().any(char::is_control)
        || !command.contains(CONTINUITY_HOOK_MARKER)
    {
        bail!("session continuity hook command is invalid");
    }
    let codex_home = fs::canonicalize(codex_home).with_context(|| {
        format!(
            "failed to resolve session continuity hook CODEX_HOME {}",
            codex_home.display()
        )
    })?;
    let path = codex_home.join("hooks.json");
    let existing = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > MAX_HOOK_CONFIG_BYTES
            {
                bail!("session continuity hook config is not a bounded regular file");
            }
            fs::read(&path).with_context(|| {
                format!("failed to read session continuity hooks {}", path.display())
            })?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let mut root = if existing.is_empty() {
        json!({"hooks": {}})
    } else {
        serde_json::from_slice::<Value>(&existing)
            .context("session continuity hooks file is invalid")?
    };
    let root = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("session continuity hooks root must be an object"))?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("session continuity hooks field must be an object"))?;
    let groups = hooks
        .entry("UserPromptSubmit")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            anyhow::anyhow!("session continuity UserPromptSubmit hooks must be an array")
        })?;
    groups.retain(|group| !group_contains_continuity_hook(group));
    groups.push(json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": 5
        }]
    }));

    let mut rendered = serde_json::to_vec_pretty(&root)?;
    rendered.push(b'\n');
    if rendered.len() as u64 > MAX_HOOK_CONFIG_BYTES {
        bail!("session continuity hooks file exceeds its size limit");
    }
    let updated = existing != rendered;
    if updated {
        install_bootstrap_atomically(&path, &rendered)?;
    }
    Ok(NativeSessionHookInstallReceipt { path, updated })
}

fn group_contains_continuity_hook(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| {
            handlers.iter().any(|handler| {
                handler
                    .get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|command| command.contains(CONTINUITY_HOOK_MARKER))
            })
        })
}

fn noop_response() -> Value {
    json!({
        "continue": true,
        "suppressOutput": true
    })
}

fn render_context(
    snapshot: &NativeSessionContinuitySnapshot,
    lane: NativeSessionLane,
    current_turn_id: &str,
) -> String {
    let lane = match lane {
        NativeSessionLane::Daily => "daily",
        NativeSessionLane::Isolated => "isolated",
    };
    let observed_age_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .map_or(0, |now| now.saturating_sub(snapshot.observed_at_unix_ms));
    format!(
        concat!(
            "Codex Administrator dual-session coordinates (machine metadata only; no message bodies).\n",
            "logical_thread={thread_id}\n",
            "current_lane={lane}\n",
            "current_turn={current_turn_id}\n",
            "relation={relation}\n",
            "common_completed_turn={common_turn}\n",
            "observed_at_unix_ms={observed_at_unix_ms}\n",
            "observed_age_ms={observed_age_ms}\n",
            "{daily}\n",
            "{isolated}\n",
            "coordination_rule=single-writer mainline; retain both heads on divergence; ",
            "handoff or merge explicitly; never last-writer-wins."
        ),
        thread_id = snapshot.continuity.thread_id,
        lane = lane,
        current_turn_id = current_turn_id,
        relation = relation_name(snapshot.continuity.relation),
        common_turn = optional(snapshot.continuity.common_turn_id.as_deref()),
        observed_at_unix_ms = snapshot.observed_at_unix_ms,
        observed_age_ms = observed_age_ms,
        daily = render_cursor("daily", &snapshot.continuity.daily_cursor),
        isolated = render_cursor("isolated", &snapshot.continuity.isolated_cursor),
    )
}

fn render_cursor(label: &str, cursor: &NativeSessionCursor) -> String {
    format!(
        concat!(
            "{label}.provider={provider}; ",
            "{label}.turn={turn}; ",
            "{label}.turn_status={turn_status}; ",
            "{label}.item={item}; ",
            "{label}.item_type={item_type}; ",
            "{label}.item_status={item_status}; ",
            "{label}.item_count={item_count}; ",
            "{label}.items_complete={items_complete}"
        ),
        label = label,
        provider = cursor.model_provider,
        turn = optional(cursor.turn_id.as_deref()),
        turn_status = cursor.turn_status.map(turn_status_name).unwrap_or("none"),
        item = optional(cursor.item_id.as_deref()),
        item_type = optional(cursor.item_type.as_deref()),
        item_status = optional(cursor.item_status.as_deref()),
        item_count = cursor.item_count,
        items_complete = cursor.items_complete,
    )
}

const fn relation_name(relation: NativeSessionRelation) -> &'static str {
    match relation {
        NativeSessionRelation::Equal => "equal",
        NativeSessionRelation::DailyAhead => "dailyAhead",
        NativeSessionRelation::IsolatedAhead => "isolatedAhead",
        NativeSessionRelation::Diverged => "diverged",
        NativeSessionRelation::Unknown => "unknown",
    }
}

const fn turn_status_name(status: NativeTurnStatus) -> &'static str {
    match status {
        NativeTurnStatus::Completed => "completed",
        NativeTurnStatus::Interrupted => "interrupted",
        NativeTurnStatus::Failed => "failed",
        NativeTurnStatus::InProgress => "inProgress",
    }
}

fn optional(value: Option<&str>) -> &str {
    value.unwrap_or("none")
}

fn validate_thread_id(thread_id: &str) -> Result<()> {
    let bytes = thread_id.as_bytes();
    if bytes.len() != 36
        || ![8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| ![8, 13, 18, 23].contains(&index) && !byte.is_ascii_hexdigit())
    {
        bail!("session continuity hook thread id is invalid");
    }
    Ok(())
}

fn validate_turn_id(turn_id: &str) -> Result<()> {
    if turn_id.trim().is_empty() || turn_id.len() > 256 || turn_id.chars().any(char::is_control) {
        bail!("session continuity hook turn id is invalid");
    }
    Ok(())
}
