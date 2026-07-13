use std::time::Duration;

use codex_administrator::{CodexAppServerClient, CodexApprovalDecision, JsonlTransport};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};

async fn read_json<R>(reader: &mut BufReader<R>) -> Value
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

async fn write_json<W>(writer: &mut W, message: Value)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    writer
        .write_all(format!("{}\n", serde_json::to_string(&message).unwrap()).as_bytes())
        .await
        .unwrap();
}

#[tokio::test]
async fn codex_client_runs_the_official_initialize_thread_turn_interrupt_sequence() {
    let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
    let (client_read, client_write) = split(client_stream);
    let (transport, _events) = JsonlTransport::spawn(client_read, client_write, 64 * 1024);
    let (server_read, mut server_write) = split(server_stream);
    let mut server_read = BufReader::new(server_read);
    let server = tokio::spawn(async move {
        let initialize = read_json(&mut server_read).await;
        assert_eq!(initialize["method"], "initialize");
        assert!(initialize.get("jsonrpc").is_none());
        write_json(
            &mut server_write,
            json!({"id":initialize["id"],"result":{"userAgent":"fixture"}}),
        )
        .await;

        assert_eq!(
            read_json(&mut server_read).await,
            json!({"method":"initialized"})
        );
        let thread = read_json(&mut server_read).await;
        assert_eq!(thread["method"], "thread/start");
        assert_eq!(thread["params"]["modelProvider"], "grok_native");
        assert_eq!(thread["params"]["model"], "grok-4");
        assert_eq!(thread["params"]["cwd"], r"D:\Work\project");
        write_json(
            &mut server_write,
            json!({"id":thread["id"],"result":{"thread":{"id":"thread-1","future":true}}}),
        )
        .await;

        let turn = read_json(&mut server_read).await;
        assert_eq!(turn["method"], "turn/start");
        assert_eq!(turn["params"]["input"][0]["text"], "Run tests");
        write_json(
            &mut server_write,
            json!({"id":turn["id"],"result":{"turn":{"id":"turn-1","status":"inProgress"}}}),
        )
        .await;

        let interrupt = read_json(&mut server_read).await;
        assert_eq!(interrupt["method"], "turn/interrupt");
        write_json(&mut server_write, json!({"id":interrupt["id"],"result":{}})).await;

        assert_eq!(
            read_json(&mut server_read).await,
            json!({"id":"approval-1","result":{"decision":"acceptForSession"}})
        );
    });

    let client = CodexAppServerClient::new(transport, Duration::from_secs(2));
    client.initialize().await.unwrap();
    let thread_id = client
        .start_thread_with_model("grok_native", "grok-4", r"D:\Work\project")
        .await
        .unwrap();
    let turn_id = client
        .start_text_turn(&thread_id, "Run tests")
        .await
        .unwrap();
    client.interrupt_turn(&thread_id, &turn_id).await.unwrap();
    client
        .respond_approval(json!("approval-1"), CodexApprovalDecision::AcceptForSession)
        .await
        .unwrap();

    assert_eq!(thread_id, "thread-1");
    assert_eq!(turn_id, "turn-1");
    server.await.unwrap();
}

#[tokio::test]
async fn codex_client_rejects_operations_before_initialization() {
    let (client_stream, _server) = tokio::io::duplex(4096);
    let (read, write) = split(client_stream);
    let (transport, _events) = JsonlTransport::spawn(read, write, 4096);
    let codex = CodexAppServerClient::new(transport, Duration::from_millis(50));

    assert!(
        codex
            .start_thread()
            .await
            .unwrap_err()
            .to_string()
            .contains("initialize")
    );
}
