use std::path::Path;

use anyhow::{Result, bail};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct GrokSessionUpdate {
    pub session_id: String,
    pub kind: String,
    pub payload: Value,
}

pub fn initialize_request(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": { "protocolVersion": 1 }
    })
}

pub fn session_new_request(id: Value, cwd: &str) -> Result<Value> {
    validate_absolute_cwd(cwd)?;
    Ok(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/new",
        "params": { "cwd": cwd, "mcpServers": [] }
    }))
}

pub fn session_load_request(id: Value, session_id: &str, cwd: &str) -> Result<Value> {
    validate_absolute_cwd(cwd)?;
    Ok(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/load",
        "params": {
            "sessionId": session_id,
            "cwd": cwd,
            "mcpServers": []
        }
    }))
}

pub fn session_prompt_text_request(id: Value, session_id: &str, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": text }]
        }
    })
}

pub fn session_cancel_notification(session_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id }
    })
}

pub fn permission_selected_response(id: Value, option_id: &str) -> Result<Value> {
    let option_id = option_id.trim();
    if option_id.is_empty() {
        bail!("permission option id cannot be blank");
    }
    Ok(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "outcome": { "outcome": "selected", "optionId": option_id }
        }
    }))
}

pub fn permission_cancelled_response(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "outcome": { "outcome": "cancelled" } }
    })
}

pub fn parse_session_id(response: &Value) -> Option<&str> {
    response
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
}

pub fn parse_session_update(message: &Value) -> Option<GrokSessionUpdate> {
    if message.get("method").and_then(Value::as_str) != Some("session/update") {
        return None;
    }
    let session_id = message.pointer("/params/sessionId")?.as_str()?.to_owned();
    let payload = message.pointer("/params/update")?.clone();
    let kind = payload.get("sessionUpdate")?.as_str()?.to_owned();
    Some(GrokSessionUpdate {
        session_id,
        kind,
        payload,
    })
}

fn validate_absolute_cwd(cwd: &str) -> Result<()> {
    if !Path::new(cwd).is_absolute() {
        bail!("Grok ACP session cwd must be absolute");
    }
    Ok(())
}
