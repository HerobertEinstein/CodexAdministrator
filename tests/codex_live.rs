use std::{collections::VecDeque, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::post,
};
use codex_administrator::{
    CodexAppServerClient, GrokNativeProviderConfig, JsonlEvent, RuntimeProcess,
    discover_codex_runtime, install_grok_native_provider_for_model,
    validate_codex_model_catalog_with_runtime,
};
use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::sync::Mutex;

#[tokio::test]
#[ignore = "requires an installed official Codex runtime"]
async fn official_codex_app_server_initializes_and_creates_a_thread() {
    let codex_home = tempdir().unwrap();
    let catalog = std::fs::canonicalize(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("grok-model-catalog.json"),
    )
    .unwrap();
    let mut spec = discover_codex_runtime().expect("official Codex CLI was not found on PATH");
    spec.env.insert(
        "CODEX_HOME".into(),
        codex_home.path().to_string_lossy().into_owned(),
    );
    spec.env.insert(
        "CODEX_ADMINISTRATOR_TEST_KEY".into(),
        "dummy-not-a-real-credential".into(),
    );
    validate_codex_model_catalog_with_runtime(&spec, &catalog, "grok-test").unwrap();
    install_grok_native_provider_for_model(
        &codex_home.path().join("config.toml"),
        &GrokNativeProviderConfig {
            base_url: "http://127.0.0.1:9/v1".into(),
            env_key: "CODEX_ADMINISTRATOR_TEST_KEY".into(),
            supports_websockets: false,
        },
        "grok-test",
        Some(&catalog),
    )
    .unwrap();
    spec.args.push("--strict-config".into());
    let mut process = RuntimeProcess::spawn(spec, 4 * 1024 * 1024).await.unwrap();
    let client = CodexAppServerClient::new(process.transport(), Duration::from_secs(15));

    let initialize = client.initialize().await.unwrap();
    let thread_id = client.start_thread().await.unwrap();

    assert!(initialize["result"]["userAgent"].is_string());
    assert!(!thread_id.trim().is_empty());
    process.terminate().await.unwrap();
}

#[derive(Clone)]
struct MockResponsesState {
    bodies: Arc<Mutex<VecDeque<String>>>,
    requests: Arc<Mutex<Vec<Value>>>,
}

async fn mock_responses(
    State(state): State<MockResponsesState>,
    Json(request): Json<Value>,
) -> Response {
    state.requests.lock().await.push(request);
    let body = state.bodies.lock().await.pop_front().unwrap_or_else(|| {
        sse(vec![json!({
            "type": "response.failed",
            "response": {
                "id": "unexpected-request",
                "error": {"code": "unexpected_request", "message": "response queue exhausted"}
            }
        })])
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    (headers, body).into_response()
}

fn sse(events: Vec<Value>) -> String {
    let mut body = String::new();
    for event in events {
        let event_type = event["type"].as_str().unwrap();
        body.push_str("event: ");
        body.push_str(event_type);
        body.push('\n');
        body.push_str("data: ");
        body.push_str(&event.to_string());
        body.push_str("\n\n");
    }
    body
}

fn response_created(id: &str) -> Value {
    json!({"type":"response.created","response":{"id":id}})
}

fn response_completed(id: &str) -> Value {
    json!({
        "type": "response.completed",
        "response": {
            "id": id,
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": null,
                "output_tokens": 0,
                "output_tokens_details": null,
                "total_tokens": 0
            }
        }
    })
}

fn value_contains(value: &Value, predicate: &impl Fn(&Value) -> bool) -> bool {
    if predicate(value) {
        return true;
    }
    match value {
        Value::Array(values) => values.iter().any(|value| value_contains(value, predicate)),
        Value::Object(values) => values
            .values()
            .any(|value| value_contains(value, predicate)),
        _ => false,
    }
}

fn find_value<'a>(value: &'a Value, predicate: &impl Fn(&Value) -> bool) -> Option<&'a Value> {
    if predicate(value) {
        return Some(value);
    }
    match value {
        Value::Array(values) => values.iter().find_map(|value| find_value(value, predicate)),
        Value::Object(values) => values
            .values()
            .find_map(|value| find_value(value, predicate)),
        _ => None,
    }
}

#[tokio::test]
#[ignore = "requires an installed official Codex runtime"]
async fn official_codex_custom_provider_keeps_the_native_tool_loop() {
    let codex_home = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let arguments = json!({
        "plan": [{"step":"Validate native provider tool loop","status":"completed"}]
    })
    .to_string();
    let state = MockResponsesState {
        bodies: Arc::new(Mutex::new(VecDeque::from([
            sse(vec![
                response_created("resp-tool"),
                json!({
                    "type": "response.output_item.done",
                    "item": {
                        "type": "function_call",
                        "call_id": "call-native-shell",
                        "name": "update_plan",
                        "arguments": arguments
                    }
                }),
                response_completed("resp-tool"),
            ]),
            sse(vec![
                response_created("resp-final"),
                json!({
                    "type": "response.output_item.added",
                    "item": {
                        "type": "message",
                        "role": "assistant",
                        "id": "msg-final",
                        "content": [{"type":"output_text","text":""}]
                    }
                }),
                json!({"type":"response.output_text.delta","delta":"native tool "}),
                json!({"type":"response.output_text.delta","delta":"completed"}),
                json!({
                    "type": "response.output_item.done",
                    "item": {
                        "type": "message",
                        "role": "assistant",
                        "id": "msg-final",
                        "content": [{"type":"output_text","text":"native tool completed"}]
                    }
                }),
                response_completed("resp-final"),
            ]),
        ]))),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server_state = state.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/responses", post(mock_responses))
                .with_state(server_state),
        )
        .await
        .unwrap();
    });

    let mut spec = discover_codex_runtime().expect("official Codex CLI was not found on PATH");
    spec.args.push("--strict-config".into());
    spec.env.insert(
        "CODEX_HOME".into(),
        codex_home.path().to_string_lossy().into_owned(),
    );
    spec.env.insert(
        "CODEX_E2E_DUMMY_KEY".into(),
        "dummy-not-a-real-credential".into(),
    );
    for value in [
        "model_providers.grok_native.name=\"Grok native provider E2E\"".to_string(),
        format!("model_providers.grok_native.base_url=\"http://{address}/v1\""),
        "model_providers.grok_native.env_key=\"CODEX_E2E_DUMMY_KEY\"".to_string(),
        "model_providers.grok_native.wire_api=\"responses\"".to_string(),
        "model_providers.grok_native.requires_openai_auth=false".to_string(),
        "model_providers.grok_native.supports_websockets=false".to_string(),
        "approval_policy=\"never\"".to_string(),
        "sandbox_mode=\"workspace-write\"".to_string(),
    ] {
        spec.args.extend(["-c".into(), value]);
    }
    let mut process = RuntimeProcess::spawn(spec, 8 * 1024 * 1024).await.unwrap();
    let client = CodexAppServerClient::new(process.transport(), Duration::from_secs(30));
    client.initialize_with_experimental_api().await.unwrap();
    let thread_id = client
        .start_thread_with_model_and_controls(
            "grok_native",
            "grok-test",
            &workspace.path().to_string_lossy(),
            "never",
            "workspace-write",
        )
        .await
        .unwrap();
    client
        .start_text_turn_with_workspace_controls(
            &thread_id,
            "Create the marker with the available shell tool",
            &workspace.path().to_string_lossy(),
        )
        .await
        .unwrap();

    let (completed, streamed_text, plan_updated) =
        tokio::time::timeout(Duration::from_secs(45), async {
            let mut streamed_text = String::new();
            let mut plan_updated = false;
            while let Some(event) = process.events_mut().recv().await {
                match event {
                    JsonlEvent::Notification { method, params }
                        if method == "item/agentMessage/delta" =>
                    {
                        streamed_text.push_str(params["delta"].as_str().unwrap_or_default());
                    }
                    JsonlEvent::Notification { method, .. } if method == "turn/plan/updated" => {
                        plan_updated = true;
                    }
                    JsonlEvent::Notification { method, params } if method == "turn/completed" => {
                        return (
                            params["turn"]["status"] == "completed",
                            streamed_text,
                            plan_updated,
                        );
                    }
                    JsonlEvent::Request { method, .. } => {
                        panic!("unexpected host approval or tool request: {method}")
                    }
                    JsonlEvent::ProtocolError { message } => panic!("protocol error: {message}"),
                    JsonlEvent::Closed { reason } => panic!("app-server closed early: {reason}"),
                    _ => {}
                }
            }
            (false, streamed_text, plan_updated)
        })
        .await
        .unwrap();

    assert!(completed);
    assert_eq!(streamed_text, "native tool completed");
    assert!(plan_updated);
    let requests = state.requests.lock().await.clone();
    assert_eq!(requests.len(), 2, "requests: {requests:#?}");
    let tool_output = find_value(&requests[1], &|value| {
        value.get("type").and_then(Value::as_str) == Some("function_call_output")
    });
    assert!(tool_output.is_some(), "missing tool output: {requests:#?}");
    assert!(value_contains(&requests[0], &|value| {
        value.as_str() == Some("update_plan")
    }));
    assert!(value_contains(&requests[1], &|value| {
        value.as_str() == Some("function_call_output")
    }));
    assert!(
        requests[1].to_string().contains("call-native-shell"),
        "second request omitted tool call identity: {}",
        requests[1]
    );
    process.terminate().await.unwrap();
    server.abort();
}

#[tokio::test]
#[ignore = "requires an installed official Codex runtime"]
async fn official_codex_custom_provider_keeps_native_image_input() {
    let codex_home = tempdir().unwrap();
    let workspace = tempdir().unwrap();
    let image_path = workspace.path().join("pixel.png");
    std::fs::write(
        &image_path,
        [
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00,
            0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ],
    )
    .unwrap();
    let state = MockResponsesState {
        bodies: Arc::new(Mutex::new(VecDeque::from([sse(vec![
            response_created("resp-image"),
            json!({
                "type": "response.output_item.done",
                "item": {
                    "type": "message",
                    "role": "assistant",
                    "id": "msg-image",
                    "content": [{"type":"output_text","text":"image received"}]
                }
            }),
            response_completed("resp-image"),
        ])]))),
        requests: Arc::new(Mutex::new(Vec::new())),
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server_state = state.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/responses", post(mock_responses))
                .with_state(server_state),
        )
        .await
        .unwrap();
    });

    let mut spec = discover_codex_runtime().expect("official Codex CLI was not found on PATH");
    spec.args.push("--strict-config".into());
    spec.env.insert(
        "CODEX_HOME".into(),
        codex_home.path().to_string_lossy().into_owned(),
    );
    spec.env.insert(
        "CODEX_E2E_DUMMY_KEY".into(),
        "dummy-not-a-real-credential".into(),
    );
    for value in [
        "model_providers.grok_native.name=\"Grok native image E2E\"".to_string(),
        format!("model_providers.grok_native.base_url=\"http://{address}/v1\""),
        "model_providers.grok_native.env_key=\"CODEX_E2E_DUMMY_KEY\"".to_string(),
        "model_providers.grok_native.wire_api=\"responses\"".to_string(),
        "model_providers.grok_native.requires_openai_auth=false".to_string(),
        "model_providers.grok_native.supports_websockets=false".to_string(),
        "approval_policy=\"never\"".to_string(),
        "sandbox_mode=\"workspace-write\"".to_string(),
    ] {
        spec.args.extend(["-c".into(), value]);
    }
    let mut process = RuntimeProcess::spawn(spec, 8 * 1024 * 1024).await.unwrap();
    let client = CodexAppServerClient::new(process.transport(), Duration::from_secs(30));
    client.initialize_with_experimental_api().await.unwrap();
    let thread_id = client
        .start_thread_with_model_and_controls(
            "grok_native",
            "grok-test",
            &workspace.path().to_string_lossy(),
            "never",
            "workspace-write",
        )
        .await
        .unwrap();
    client
        .start_text_and_local_image_turn(
            &thread_id,
            "Describe this image",
            &image_path.to_string_lossy(),
        )
        .await
        .unwrap();

    let completed = tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(event) = process.events_mut().recv().await {
            match event {
                JsonlEvent::Notification { method, params } if method == "turn/completed" => {
                    return params["turn"]["status"] == "completed";
                }
                JsonlEvent::Request { method, .. } => {
                    panic!("unexpected host request during image turn: {method}")
                }
                JsonlEvent::ProtocolError { message } => panic!("protocol error: {message}"),
                JsonlEvent::Closed { reason } => panic!("app-server closed early: {reason}"),
                _ => {}
            }
        }
        false
    })
    .await
    .unwrap();

    assert!(completed);
    let requests = state.requests.lock().await.clone();
    assert_eq!(requests.len(), 1, "requests: {requests:#?}");
    assert!(
        value_contains(&requests[0], &|value| {
            value.as_str() == Some("input_image")
        }),
        "request omitted input_image: {:#?}",
        requests[0]
    );
    assert!(
        requests[0].to_string().contains("data:image/png;base64,"),
        "request omitted PNG data URL: {:#?}",
        requests[0]
    );

    process.terminate().await.unwrap();
    server.abort();
}
