use codex_administrator::{ControlOperation, ControlResponse, parse_control_requests};
use serde_json::json;

const NONCE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn parses_only_versioned_nonce_bound_whitelisted_requests() {
    let mut value = json!([{
        "version": 1,
        "id": "ca-1",
        "nonce": NONCE,
        "operation": "models.discover",
        "payload": {
            "base_url": "https://ai.hebox.net/v1",
            "credential": "transient-only"
        }
    }]);

    let mut requests = parse_control_requests(&mut value, NONCE).unwrap();
    assert_eq!(requests.len(), 1);
    let request = requests.pop().unwrap();
    assert_eq!(request.id(), "ca-1");
    assert_eq!(request.operation(), ControlOperation::ModelsDiscover);
    let payload = request.into_payload();
    assert_eq!(payload["credential"], "transient-only");
    assert_eq!(value, json!(null));
}

#[test]
fn rejects_wrong_nonce_unknown_operations_unknown_fields_and_oversized_payloads() {
    let mut wrong_nonce = json!([{
        "version": 1,
        "id": "ca-1",
        "nonce": "wrong",
        "operation": "state.read",
        "payload": {}
    }]);
    assert!(parse_control_requests(&mut wrong_nonce, NONCE).is_err());

    let mut unknown_operation = json!([{
        "version": 1,
        "id": "ca-1",
        "nonce": NONCE,
        "operation": "credential.get",
        "payload": {}
    }]);
    assert!(parse_control_requests(&mut unknown_operation, NONCE).is_err());

    let mut unknown_field = json!([{
        "version": 1,
        "id": "ca-1",
        "nonce": NONCE,
        "operation": "state.read",
        "payload": {},
        "unexpected": true
    }]);
    assert!(parse_control_requests(&mut unknown_field, NONCE).is_err());

    let mut oversized = json!([{
        "version": 1,
        "id": "ca-1",
        "nonce": NONCE,
        "operation": "config.apply",
        "payload": { "value": "x".repeat(70_000) }
    }]);
    assert!(parse_control_requests(&mut oversized, NONCE).is_err());
}

#[test]
fn control_responses_never_echo_request_payloads() {
    let success =
        ControlResponse::success("ca-1", NONCE, json!({ "credential_present": true })).into_value();
    assert_eq!(success["ok"], true);
    assert_eq!(success["result"]["credential_present"], true);
    assert!(success.get("payload").is_none());
    assert!(success.to_string().find("transient-only").is_none());

    let error = ControlResponse::error("ca-2", NONCE, "model refresh failed").into_value();
    assert_eq!(error["ok"], false);
    assert_eq!(error["error"], "model refresh failed");
    assert!(error.get("result").is_none());
}
