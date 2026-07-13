use serde_json::json;

#[path = "../src/protocol/grok.rs"]
mod grok;

use grok::{
    initialize_request, parse_session_id, parse_session_update, permission_cancelled_response,
    permission_selected_response, session_cancel_notification, session_load_request,
    session_new_request, session_prompt_text_request,
};

#[test]
fn constructs_minimal_acp_v1_initialize_with_jsonrpc() {
    assert_eq!(
        initialize_request(json!("ca:init:1")),
        json!({
            "jsonrpc": "2.0",
            "id": "ca:init:1",
            "method": "initialize",
            "params": { "protocolVersion": 1 }
        })
    );
}

#[test]
fn session_new_and_load_require_absolute_cwd_and_explicit_empty_mcp_servers() {
    assert_eq!(
        session_new_request(json!("ca:new:1"), r"D:\repo").unwrap(),
        json!({
            "jsonrpc": "2.0",
            "id": "ca:new:1",
            "method": "session/new",
            "params": { "cwd": r"D:\repo", "mcpServers": [] }
        })
    );
    assert_eq!(
        session_load_request(json!("ca:load:1"), "session-1", r"D:\repo").unwrap(),
        json!({
            "jsonrpc": "2.0",
            "id": "ca:load:1",
            "method": "session/load",
            "params": {
                "sessionId": "session-1",
                "cwd": r"D:\repo",
                "mcpServers": []
            }
        })
    );
    assert!(session_new_request(json!(1), "relative").is_err());
}

#[test]
fn prompt_and_cancel_follow_acp_request_and_notification_shapes() {
    assert_eq!(
        session_prompt_text_request(json!("ca:prompt:1"), "session-1", "Run tests"),
        json!({
            "jsonrpc": "2.0",
            "id": "ca:prompt:1",
            "method": "session/prompt",
            "params": {
                "sessionId": "session-1",
                "prompt": [{ "type": "text", "text": "Run tests" }]
            }
        })
    );
    let cancel = session_cancel_notification("session-1");
    assert_eq!(
        cancel,
        json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": "session-1" }
        })
    );
    assert!(cancel.get("id").is_none());
}

#[test]
fn permission_responses_preserve_server_ids_and_option_ids() {
    assert_eq!(
        permission_selected_response(json!(17), "once-17").unwrap(),
        json!({
            "jsonrpc": "2.0",
            "id": 17,
            "result": {
                "outcome": { "outcome": "selected", "optionId": "once-17" }
            }
        })
    );
    assert_eq!(
        permission_cancelled_response(json!("permission-2")),
        json!({
            "jsonrpc": "2.0",
            "id": "permission-2",
            "result": { "outcome": { "outcome": "cancelled" } }
        })
    );
    assert!(permission_selected_response(json!(17), " ").is_err());
}

#[test]
fn parsers_extract_session_ids_and_tolerate_unknown_update_types() {
    assert_eq!(
        parse_session_id(&json!({
            "jsonrpc": "2.0",
            "id": "ca:new:1",
            "result": { "sessionId": "session-1", "future": true }
        })),
        Some("session-1")
    );

    let message = json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "session-1",
            "update": {
                "sessionUpdate": "future_update_kind",
                "futurePayload": { "keep": true }
            }
        }
    });
    let update = parse_session_update(&message).unwrap();
    assert_eq!(update.session_id, "session-1");
    assert_eq!(update.kind, "future_update_kind");
    assert_eq!(update.payload["futurePayload"]["keep"], true);
}
