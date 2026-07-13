use std::time::Duration;

use codex_administrator::{CodexAppServerClient, RuntimeProcess, discover_codex_runtime};

#[tokio::test]
#[ignore = "requires an installed official Codex runtime"]
async fn official_codex_app_server_initializes_and_creates_a_thread() {
    let spec = discover_codex_runtime().expect("official Codex CLI was not found on PATH");
    let mut process = RuntimeProcess::spawn(spec, 4 * 1024 * 1024).await.unwrap();
    let client = CodexAppServerClient::new(process.transport(), Duration::from_secs(15));

    let initialize = client.initialize().await.unwrap();
    let thread_id = client.start_thread().await.unwrap();

    assert!(initialize["result"]["userAgent"].is_string());
    assert!(!thread_id.trim().is_empty());
    process.terminate().await.unwrap();
}
