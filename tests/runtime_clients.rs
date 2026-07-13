use std::time::Duration;

use codex_administrator::{
    CodexAppServerClient, CodexApprovalDecision, GrokAcpClient, JsonlTransport,
};
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
    let thread_id = client.start_thread().await.unwrap();
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
async fn grok_client_runs_acp_initialize_session_prompt_cancel_and_permission_sequence() {
    let (client_stream, server_stream) = tokio::io::duplex(64 * 1024);
    let (client_read, client_write) = split(client_stream);
    let (transport, _events) = JsonlTransport::spawn(client_read, client_write, 64 * 1024);
    let (server_read, mut server_write) = split(server_stream);
    let mut server_read = BufReader::new(server_read);
    let server = tokio::spawn(async move {
        let initialize = read_json(&mut server_read).await;
        assert_eq!(initialize["jsonrpc"], "2.0");
        assert_eq!(initialize["method"], "initialize");
        write_json(
            &mut server_write,
            json!({"jsonrpc":"2.0","id":initialize["id"],"result":{"protocolVersion":1}}),
        )
        .await;

        let new_session = read_json(&mut server_read).await;
        assert_eq!(new_session["method"], "session/new");
        write_json(
            &mut server_write,
            json!({"jsonrpc":"2.0","id":new_session["id"],"result":{"sessionId":"session-1"}}),
        )
        .await;

        let prompt = read_json(&mut server_read).await;
        assert_eq!(prompt["method"], "session/prompt");
        write_json(
            &mut server_write,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;

        let cancel = read_json(&mut server_read).await;
        assert_eq!(cancel["method"], "session/cancel");
        assert!(cancel.get("id").is_none());
        assert_eq!(
            read_json(&mut server_read).await,
            json!({
                "jsonrpc":"2.0",
                "id":17,
                "result":{"outcome":{"outcome":"selected","optionId":"once-17"}}
            })
        );
    });

    let client = GrokAcpClient::new(transport, Duration::from_secs(2));
    client.initialize().await.unwrap();
    let session_id = client.new_session(r"D:\repo").await.unwrap();
    assert_eq!(
        client.prompt_text(&session_id, "Run tests").await.unwrap(),
        "end_turn"
    );
    client.cancel(&session_id).await.unwrap();
    client
        .select_permission(json!(17), "once-17")
        .await
        .unwrap();

    assert_eq!(session_id, "session-1");
    server.await.unwrap();
}

#[tokio::test]
async fn runtime_clients_reject_operations_before_initialization() {
    let (codex_stream, _server) = tokio::io::duplex(4096);
    let (read, write) = split(codex_stream);
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

    let (grok_stream, _server) = tokio::io::duplex(4096);
    let (read, write) = split(grok_stream);
    let (transport, _events) = JsonlTransport::spawn(read, write, 4096);
    let grok = GrokAcpClient::new(transport, Duration::from_millis(50));
    assert!(
        grok.new_session(r"D:\repo")
            .await
            .unwrap_err()
            .to_string()
            .contains("initialize")
    );
}
