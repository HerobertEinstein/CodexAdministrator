use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

use codex_administrator::{ControlOperation, ControlResponse, DirectCdpTarget, LoopbackCdpClient};
use serde_json::{Value, json};
use tungstenite::{Message, accept};

fn spawn_http_once(path: &'static str, body: String) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.starts_with(&format!("GET {path} HTTP/1.1")));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    (port, handle)
}

fn target(port: u16) -> DirectCdpTarget {
    DirectCdpTarget {
        id: "official".into(),
        page_url: "app://-/index.html".into(),
        websocket_url: format!("ws://127.0.0.1:{port}/devtools/page/official"),
    }
}

fn read_command(socket: &mut tungstenite::WebSocket<std::net::TcpStream>) -> Value {
    let message = socket.read().unwrap();
    serde_json::from_str(message.to_text().unwrap()).unwrap()
}

#[test]
fn target_discovery_selects_only_the_official_app_renderer() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let body = json!([
        {
            "id": "web",
            "type": "page",
            "url": "https://example.com",
            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/web")
        },
        {
            "id": "official",
            "type": "page",
            "url": "app://-/index.html",
            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/official")
        }
    ])
    .to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..read]);
        assert!(request.starts_with("GET /json/list HTTP/1.1"));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let discovered = LoopbackCdpClient::default()
        .wait_for_app_target(port, Duration::from_secs(1))
        .unwrap();

    assert_eq!(discovered, target(port));
    server.join().unwrap();
}

#[test]
fn endpoint_wait_uses_only_the_owned_loopback_port() {
    let (port, server) = spawn_http_once("/json/version", "{}".into());

    LoopbackCdpClient::default()
        .wait_for_endpoint(port, Duration::from_secs(1))
        .unwrap();

    server.join().unwrap();
}

#[test]
fn bootstrap_install_waits_for_a_healthy_provider_on_the_same_websocket() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();

        let inject = read_command(&mut socket);
        assert_eq!(inject["method"], "Runtime.evaluate");
        assert_eq!(inject["params"]["expression"], "bootstrap();");
        socket
            .send(Message::Text(
                json!({
                    "id": inject["id"],
                    "result": {"result": {"type": "undefined"}}
                })
                .to_string()
                .into(),
            ))
            .unwrap();

        let health = read_command(&mut socket);
        assert_eq!(health["method"], "Runtime.evaluate");
        assert!(
            health["params"]["expression"]
                .as_str()
                .unwrap()
                .contains("__codexAdministrator")
        );
        socket
            .send(Message::Text(
                json!({
                    "id": health["id"],
                    "result": {
                        "result": {
                            "type": "object",
                            "value": {"ok": true, "provider": "grok_native"}
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        socket.close(None).unwrap();
    });

    LoopbackCdpClient::default()
        .install_bootstrap(&target(port), "bootstrap();", Duration::from_secs(1))
        .unwrap();

    server.join().unwrap();
}

#[test]
fn bootstrap_reinstalls_after_the_renderer_context_resets_during_startup() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();

        let first_inject = read_command(&mut socket);
        assert_eq!(first_inject["params"]["expression"], "bootstrap();");
        socket
            .send(Message::Text(
                json!({
                    "id": first_inject["id"],
                    "result": {"result": {"type": "undefined"}}
                })
                .to_string()
                .into(),
            ))
            .unwrap();

        let missing_health = read_command(&mut socket);
        assert!(
            missing_health["params"]["expression"]
                .as_str()
                .unwrap()
                .contains("__codexAdministrator")
        );
        socket
            .send(Message::Text(
                json!({
                    "id": missing_health["id"],
                    "result": {"result": {"type": "object", "value": null}}
                })
                .to_string()
                .into(),
            ))
            .unwrap();

        let reinject = read_command(&mut socket);
        assert_eq!(reinject["params"]["expression"], "bootstrap();");
        socket
            .send(Message::Text(
                json!({
                    "id": reinject["id"],
                    "result": {"result": {"type": "undefined"}}
                })
                .to_string()
                .into(),
            ))
            .unwrap();

        let healthy = read_command(&mut socket);
        socket
            .send(Message::Text(
                json!({
                    "id": healthy["id"],
                    "result": {
                        "result": {
                            "type": "object",
                            "value": {"ok": true, "provider": "grok_native"}
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
        socket.close(None).unwrap();
    });

    LoopbackCdpClient::default()
        .install_bootstrap(&target(port), "bootstrap();", Duration::from_secs(1))
        .unwrap();

    server.join().unwrap();
}

#[test]
fn cdp_protocol_errors_fail_closed_instead_of_claiming_injection() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        let command = read_command(&mut socket);
        socket
            .send(Message::Text(
                json!({
                    "id": command["id"],
                    "error": {"code": -32601, "message": "Runtime.evaluate unavailable"}
                })
                .to_string()
                .into(),
            ))
            .unwrap();
    });

    let error = LoopbackCdpClient::default()
        .install_bootstrap(&target(port), "bootstrap();", Duration::from_secs(1))
        .unwrap_err();

    assert!(error.to_string().contains("Runtime.evaluate unavailable"));
    server.join().unwrap();
}

#[test]
fn ui_readiness_waits_until_the_native_renderer_has_interactive_content() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        for ready in [false, true] {
            let command = read_command(&mut socket);
            assert_eq!(command["method"], "Runtime.evaluate");
            assert!(
                command["params"]["expression"]
                    .as_str()
                    .unwrap()
                    .contains("querySelector")
            );
            socket
                .send(Message::Text(
                    json!({
                        "id": command["id"],
                        "result": {
                            "result": {"type": "boolean", "value": ready}
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .unwrap();
        }
    });

    LoopbackCdpClient::default()
        .wait_for_ui_ready(&target(port), Duration::from_secs(1))
        .unwrap();

    server.join().unwrap();
}

#[test]
fn provider_readiness_fails_closed_when_the_app_server_did_not_load_grok() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        let command = read_command(&mut socket);
        let expression = command["params"]["expression"].as_str().unwrap();
        assert!(expression.contains("config/read"));
        assert!(expression.contains("grok_native"));
        socket
            .send(Message::Text(
                json!({
                    "id": command["id"],
                    "result": {
                        "result": {
                            "type": "object",
                            "value": {
                                "ok": false,
                                "error": "model provider 'grok_native' not found"
                            }
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
    });

    let error = LoopbackCdpClient::default()
        .wait_for_provider_ready(&target(port), Duration::from_secs(1))
        .unwrap_err();

    assert!(error.to_string().contains("grok_native"));
    server.join().unwrap();
}

#[test]
fn provider_readiness_accepts_only_the_loaded_grok_provider() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        let command = read_command(&mut socket);
        socket
            .send(Message::Text(
                json!({
                    "id": command["id"],
                    "result": {
                        "result": {
                            "type": "object",
                            "value": {"ok": true, "provider": "grok_native"}
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
    });

    LoopbackCdpClient::default()
        .wait_for_provider_ready(&target(port), Duration::from_secs(1))
        .unwrap();

    server.join().unwrap();
}

#[test]
fn control_requests_are_drained_through_the_isolated_renderer_only() {
    const NONCE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        let command = read_command(&mut socket);
        let expression = command["params"]["expression"].as_str().unwrap();
        assert!(expression.contains("__codexAdministratorControlInternal"));
        assert!(expression.contains(".drain"));
        assert!(expression.contains(NONCE));
        socket
            .send(Message::Text(
                json!({
                    "id": command["id"],
                    "result": {
                        "result": {
                            "type": "object",
                            "value": [{
                                "version": 1,
                                "id": "ca-1",
                                "nonce": NONCE,
                                "operation": "state.read",
                                "payload": {}
                            }]
                        }
                    }
                })
                .to_string()
                .into(),
            ))
            .unwrap();
    });

    let requests = LoopbackCdpClient::default()
        .drain_control_requests(&target(port), NONCE)
        .unwrap();

    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id(), "ca-1");
    assert_eq!(requests[0].operation(), ControlOperation::StateRead);
    server.join().unwrap();
}

#[test]
fn control_responses_are_delivered_without_embedding_request_payloads() {
    const NONCE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = accept(stream).unwrap();
        let command = read_command(&mut socket);
        let expression = command["params"]["expression"].as_str().unwrap();
        assert!(expression.contains("__codexAdministratorControlInternal"));
        assert!(expression.contains(".deliver"));
        assert!(expression.contains("credential_present"));
        assert!(!expression.contains("transient-only"));
        socket
            .send(Message::Text(
                json!({
                    "id": command["id"],
                    "result": {"result": {"type": "boolean", "value": true}}
                })
                .to_string()
                .into(),
            ))
            .unwrap();
    });

    LoopbackCdpClient::default()
        .deliver_control_response(
            &target(port),
            ControlResponse::success("ca-1", NONCE, json!({"credential_present": true})),
        )
        .unwrap();

    server.join().unwrap();
}
