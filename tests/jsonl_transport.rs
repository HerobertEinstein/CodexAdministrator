use std::time::Duration;

use codex_administrator::{JsonlEvent, JsonlTransport};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};

#[tokio::test]
async fn writes_exactly_one_json_object_per_line_without_content_length() {
    let (client, server) = tokio::io::duplex(4096);
    let (client_read, client_write) = split(client);
    let (transport, _events) = JsonlTransport::spawn(client_read, client_write, 4096);
    let (server_read, _server_write) = split(server);
    let mut server_read = BufReader::new(server_read);

    transport
        .notify(json!({"method":"initialized"}))
        .await
        .unwrap();
    let mut line = String::new();
    server_read.read_line(&mut line).await.unwrap();

    assert_eq!(line, "{\"method\":\"initialized\"}\n");
    assert!(!line.contains("Content-Length"));
}

#[tokio::test]
async fn correlates_string_and_numeric_request_ids_when_responses_arrive_out_of_order() {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let (client_read, client_write) = split(client);
    let (transport, _events) = JsonlTransport::spawn(client_read, client_write, 4096);
    let (server_read, mut server_write) = split(server);
    let mut server_read = BufReader::new(server_read);

    let first = tokio::spawn({
        let transport = transport.clone();
        async move {
            transport
                .request(
                    json!({"id":"one","method":"first","params":{}}),
                    Duration::from_secs(2),
                )
                .await
        }
    });
    let second = tokio::spawn({
        let transport = transport.clone();
        async move {
            transport
                .request(
                    json!({"id":2,"method":"second","params":{}}),
                    Duration::from_secs(2),
                )
                .await
        }
    });

    let mut lines = Vec::new();
    for _ in 0..2 {
        let mut line = String::new();
        server_read.read_line(&mut line).await.unwrap();
        assert!(line.ends_with('\n'));
        assert!(!line.contains("Content-Length"));
        lines.push(serde_json::from_str::<Value>(line.trim()).unwrap());
    }
    assert!(lines.iter().any(|message| message["id"] == "one"));
    assert!(lines.iter().any(|message| message["id"] == 2));

    server_write
        .write_all(b"{\"id\":2,\"result\":{\"order\":1}}\n")
        .await
        .unwrap();
    server_write
        .write_all(b"{\"id\":\"one\",\"result\":{\"order\":2}}\n")
        .await
        .unwrap();

    assert_eq!(second.await.unwrap().unwrap()["result"]["order"], 1);
    assert_eq!(first.await.unwrap().unwrap()["result"]["order"], 2);
}

#[tokio::test]
async fn forwards_server_requests_and_notifications_without_blocking_pending_responses() {
    let (client, server) = tokio::io::duplex(16 * 1024);
    let (client_read, client_write) = split(client);
    let (transport, mut events) = JsonlTransport::spawn(client_read, client_write, 4096);
    let (server_read, mut server_write) = split(server);
    let mut server_read = BufReader::new(server_read);

    let pending = tokio::spawn({
        let transport = transport.clone();
        async move {
            transport
                .request(
                    json!({"id":"pending","method":"run","params":{}}),
                    Duration::from_secs(2),
                )
                .await
        }
    });
    let mut request_line = String::new();
    server_read.read_line(&mut request_line).await.unwrap();

    server_write
        .write_all(b"{\"id\":17,\"method\":\"session/request_permission\",\"params\":{}}\n")
        .await
        .unwrap();
    server_write
        .write_all(b"{\"method\":\"session/update\",\"params\":{\"kind\":\"chunk\"}}\n")
        .await
        .unwrap();
    server_write
        .write_all(b"{\"id\":\"pending\",\"result\":{}}\n")
        .await
        .unwrap();

    match events.recv().await.unwrap() {
        JsonlEvent::Request { id, method, .. } => {
            assert_eq!(id, json!(17));
            assert_eq!(method, "session/request_permission");
        }
        event => panic!("unexpected event: {event:?}"),
    }
    match events.recv().await.unwrap() {
        JsonlEvent::Notification { method, .. } => assert_eq!(method, "session/update"),
        event => panic!("unexpected event: {event:?}"),
    }
    pending.await.unwrap().unwrap();
}

#[tokio::test]
async fn timed_out_request_ids_can_be_reused_without_leaking_correlation_state() {
    let (client, _server) = tokio::io::duplex(4096);
    let (client_read, client_write) = split(client);
    let (transport, _events) = JsonlTransport::spawn(client_read, client_write, 4096);
    let message = json!({"id":"same","method":"slow","params":{}});

    let first = transport
        .request(message.clone(), Duration::from_millis(10))
        .await;
    let second = transport.request(message, Duration::from_millis(10)).await;

    assert!(first.unwrap_err().to_string().contains("timed out"));
    assert!(second.unwrap_err().to_string().contains("timed out"));
}

#[tokio::test]
async fn rejects_outbound_messages_without_an_id_as_requests() {
    let (client, _server) = tokio::io::duplex(4096);
    let (client_read, client_write) = split(client);
    let (transport, _events) = JsonlTransport::spawn(client_read, client_write, 4096);

    let error = transport
        .request(json!({"method":"missing-id"}), Duration::from_secs(1))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("request id"));
}
