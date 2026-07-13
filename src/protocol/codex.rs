use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientInfo<'a> {
    pub name: &'a str,
    pub title: &'a str,
    pub version: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

impl ApprovalDecision {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "accept",
            Self::AcceptForSession => "acceptForSession",
            Self::Decline => "decline",
            Self::Cancel => "cancel",
        }
    }
}

pub fn initialize_request(id: Value, client: ClientInfo<'_>) -> Value {
    json!({
        "id": id,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": client.name,
                "title": client.title,
                "version": client.version,
            }
        }
    })
}

pub fn initialized_notification() -> Value {
    json!({ "method": "initialized" })
}

pub fn thread_start_request(id: Value) -> Value {
    json!({
        "id": id,
        "method": "thread/start",
        "params": {}
    })
}

pub fn thread_resume_request(id: Value, thread_id: &str) -> Value {
    json!({
        "id": id,
        "method": "thread/resume",
        "params": { "threadId": thread_id }
    })
}

pub fn turn_start_text_request(id: Value, thread_id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "method": "turn/start",
        "params": {
            "threadId": thread_id,
            "input": [{ "type": "text", "text": text }]
        }
    })
}

pub fn turn_interrupt_request(id: Value, thread_id: &str, turn_id: &str) -> Value {
    json!({
        "id": id,
        "method": "turn/interrupt",
        "params": {
            "threadId": thread_id,
            "turnId": turn_id
        }
    })
}

pub fn approval_response(id: Value, decision: ApprovalDecision) -> Value {
    json!({
        "id": id,
        "result": { "decision": decision.as_str() }
    })
}

pub fn parse_thread_id(response: &Value) -> Option<&str> {
    response
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
}

pub fn parse_turn_id(response: &Value) -> Option<&str> {
    response.pointer("/result/turn/id").and_then(Value::as_str)
}

pub fn encode_json_line(message: &Value) -> serde_json::Result<String> {
    serde_json::to_string(message).map(|mut line| {
        line.push('\n');
        line
    })
}
