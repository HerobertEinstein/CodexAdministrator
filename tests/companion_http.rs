use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use codex_administrator::{AgentMode, CompanionContext, build_companion_router};
use serde_json::{Value, json};
use tower::ServiceExt;

const CAPABILITY: &str = "0123456789abcdef0123456789abcdef";

fn app() -> axum::Router {
    build_companion_router(CompanionContext::new(CAPABILITY).unwrap())
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn state_endpoints_reject_unauthenticated_requests_without_cors() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/api/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[tokio::test]
async fn bearer_authorization_reads_and_updates_the_model_selection_intent() {
    let app = app();
    let state_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/state")
                .header(header::AUTHORIZATION, format!("Bearer {CAPABILITY}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(state_response.status(), StatusCode::OK);
    assert_eq!(
        response_json(state_response).await["mode"],
        "native_gpt_main"
    );

    let update_response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/state/mode")
                .header(header::AUTHORIZATION, format!("Bearer {CAPABILITY}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "mode": AgentMode::GrokNativeModel }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);
    let state = response_json(update_response).await;
    assert_eq!(state["mode"], "grok_native_model");
    assert_eq!(state["revision"], 1);
}

#[tokio::test]
async fn iframe_ui_is_static_and_contains_no_launch_capability() {
    let response = app()
        .oneshot(Request::builder().uri("/ui/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=utf-8"
    );
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("Grok"));
    assert!(html.contains("/ui/app.js"));
    assert!(!html.contains(CAPABILITY));
}

#[tokio::test]
async fn iframe_application_keeps_the_fragment_capability_in_memory_only() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/ui/app.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/javascript; charset=utf-8"
    );
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let script = String::from_utf8(body.to_vec()).unwrap();
    assert!(script.contains("postMessage"));
    assert!(script.contains("location.hash"));
    assert!(script.contains("history.replaceState"));
    assert!(script.contains("Authorization"));
    assert!(!script.contains(CAPABILITY));
}
