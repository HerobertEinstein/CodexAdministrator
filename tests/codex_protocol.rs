use serde_json::{Value, json};

#[path = "../src/protocol/codex.rs"]
mod codex;

use codex::{
    ApprovalDecision, ClientInfo, approval_response, encode_json_line, initialize_request,
    initialized_notification, parse_thread_id, parse_turn_id, thread_resume_request,
    thread_start_request, turn_interrupt_request, turn_start_text_request,
};

#[test]
fn constructs_initialize_handshake_without_jsonrpc_header() {
    let initialize = initialize_request(
        json!(0),
        ClientInfo {
            name: "codex_administrator",
            title: "Codex Administrator",
            version: "0.1.0-alpha.1",
        },
    );

    assert_eq!(
        initialize,
        json!({
            "method": "initialize",
            "id": 0,
            "params": {
                "clientInfo": {
                    "name": "codex_administrator",
                    "title": "Codex Administrator",
                    "version": "0.1.0-alpha.1"
                }
            }
        })
    );
    assert_eq!(
        initialized_notification(),
        json!({ "method": "initialized" })
    );
    assert!(initialize.get("jsonrpc").is_none());
}

#[test]
fn constructs_thread_start_and_resume_requests() {
    assert_eq!(
        thread_start_request(json!(10)),
        json!({
            "method": "thread/start",
            "id": 10,
            "params": {}
        })
    );
    assert_eq!(
        thread_resume_request(json!(11), "thr_123"),
        json!({
            "method": "thread/resume",
            "id": 11,
            "params": { "threadId": "thr_123" }
        })
    );
}

#[test]
fn constructs_experimental_initialize_capability_negotiation() {
    let request = codex::initialize_request_with_experimental_api(
        json!(2),
        codex::ClientInfo {
            name: "test-client",
            title: "Test Client",
            version: "1.0.0",
        },
    );

    assert_eq!(request["params"]["capabilities"]["experimentalApi"], true);
    assert!(request.get("jsonrpc").is_none());
}

#[test]
fn constructs_a_native_provider_thread_without_replacing_host_controls() {
    let request = codex::thread_start_with_model_request(
        json!(3),
        "grok_native",
        "grok-4",
        r"D:\Work\project",
    );

    assert_eq!(request["method"], "thread/start");
    assert_eq!(request["params"]["modelProvider"], "grok_native");
    assert_eq!(request["params"]["model"], "grok-4");
    assert_eq!(request["params"]["cwd"], r"D:\Work\project");
    assert!(request["params"].get("tools").is_none());
    assert!(request["params"].get("sandbox").is_none());
    assert!(request.get("jsonrpc").is_none());
}

#[test]
fn constructs_a_native_provider_thread_with_explicit_host_controls() {
    let request = codex::thread_start_with_model_and_controls_request(
        json!(4),
        "grok_native",
        "grok-4",
        r"D:\Work\project",
        "never",
        "workspace-write",
    );

    assert_eq!(request["params"]["modelProvider"], "grok_native");
    assert_eq!(request["params"]["approvalPolicy"], "never");
    assert_eq!(request["params"]["sandbox"], "workspace-write");
    assert_eq!(request["params"]["environments"], json!([]));
}

#[test]
fn constructs_text_turn_start_and_interrupt_requests() {
    assert_eq!(
        turn_start_text_request(json!(30), "thr_123", "Run tests"),
        json!({
            "method": "turn/start",
            "id": 30,
            "params": {
                "threadId": "thr_123",
                "input": [{ "type": "text", "text": "Run tests" }]
            }
        })
    );
    assert_eq!(
        turn_interrupt_request(json!(31), "thr_123", "turn_456"),
        json!({
            "method": "turn/interrupt",
            "id": 31,
            "params": {
                "threadId": "thr_123",
                "turnId": "turn_456"
            }
        })
    );
}

#[test]
fn constructs_a_native_local_image_turn() {
    let request = codex::turn_start_text_and_local_image_request(
        json!(8),
        "thread-1",
        "Describe this image",
        r"D:\Work\project\pixel.png",
    );

    assert_eq!(request["method"], "turn/start");
    assert_eq!(request["params"]["input"][0]["type"], "text");
    assert_eq!(request["params"]["input"][1]["type"], "localImage");
    assert_eq!(
        request["params"]["input"][1]["path"],
        r"D:\Work\project\pixel.png"
    );
}

#[test]
fn constructs_a_text_turn_with_native_workspace_controls() {
    let request = codex::turn_start_text_with_workspace_controls_request(
        json!(9),
        "thread-1",
        "Run the tool",
        r"D:\Work\project",
    );

    assert_eq!(request["params"]["approvalPolicy"], "never");
    assert_eq!(request["params"]["sandboxPolicy"]["type"], "workspaceWrite");
    assert_eq!(
        request["params"]["sandboxPolicy"]["writableRoots"][0],
        r"D:\Work\project"
    );
    assert_eq!(request["params"]["environments"], json!([]));
}

#[test]
fn approval_responses_preserve_string_and_numeric_request_ids() {
    let cases = [
        (json!("approval-1"), ApprovalDecision::Accept, "accept"),
        (
            json!(9_007_199_254_740_991_i64),
            ApprovalDecision::AcceptForSession,
            "acceptForSession",
        ),
        (json!("approval-3"), ApprovalDecision::Decline, "decline"),
        (json!(-42), ApprovalDecision::Cancel, "cancel"),
    ];

    for (id, decision, expected_decision) in cases {
        let response = approval_response(id.clone(), decision);

        assert_eq!(response["id"], id);
        assert_eq!(response["result"]["decision"], expected_decision);
        assert!(response.get("jsonrpc").is_none());
    }
}

#[test]
fn response_parsers_extract_only_ids_and_tolerate_extra_fields() {
    let thread_response = json!({
        "id": 10,
        "result": {
            "thread": {
                "id": "thr_123",
                "status": "idle",
                "futureThreadField": { "nested": true }
            },
            "futureResultField": [1, 2, 3]
        },
        "futureEnvelopeField": "ignored"
    });
    let turn_response = json!({
        "id": 30,
        "result": {
            "turn": {
                "id": "turn_456",
                "status": "inProgress",
                "items": []
            },
            "anotherFutureField": true
        }
    });

    assert_eq!(parse_thread_id(&thread_response), Some("thr_123"));
    assert_eq!(parse_turn_id(&turn_response), Some("turn_456"));
    assert_eq!(parse_thread_id(&json!({ "result": {} })), None);
    assert_eq!(parse_turn_id(&json!({ "result": { "turn": {} } })), None);
}

#[test]
fn jsonl_encoder_emits_exactly_one_json_line() {
    let message = turn_start_text_request(json!(30), "thr_123", "first\nsecond");
    let encoded = encode_json_line(&message).expect("message should serialize");

    assert!(encoded.ends_with('\n'));
    assert_eq!(encoded.lines().count(), 1);
    assert!(!encoded.contains("\"jsonrpc\""));
    assert_eq!(
        serde_json::from_str::<Value>(encoded.trim_end()).expect("line should be valid JSON"),
        message
    );
}
