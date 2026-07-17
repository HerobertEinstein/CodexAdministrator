use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use codex_administrator::{
    fetch_model_list, injectable_grok_models, model_list_url, parse_model_list, search_models,
};

#[test]
fn model_list_url_uses_the_openai_compatible_models_endpoint() {
    assert_eq!(
        model_list_url("https://example.com/v1").unwrap(),
        "https://example.com/v1/models"
    );
    assert_eq!(
        model_list_url("https://example.com/v1/").unwrap(),
        "https://example.com/v1/models"
    );
    assert!(model_list_url("https://example.com/v1?token=secret").is_err());
    assert!(model_list_url("http://example.com/v1").is_err());
    assert!(model_list_url("https://user:secret@example.com/v1").is_err());
}

#[test]
fn model_list_parser_deduplicates_and_rejects_invalid_entries_without_leaking_payloads() {
    let models = parse_model_list(
        br#"{
            "object":"list",
            "data":[
                {"id":"grok-4.5","owned_by":"xai"},
                {"id":"grok-4.5","owned_by":"duplicate"},
                {"id":"grok-4.3-high","owned_by":"custom"}
            ]
        }"#,
    )
    .unwrap();

    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "grok-4.5");
    assert_eq!(models[0].owned_by.as_deref(), Some("xai"));
    assert_eq!(models[1].id, "grok-4.3-high");

    let error = parse_model_list(br#"{"data":[{"id":"bad\nmodel"}]}"#).unwrap_err();
    assert!(!error.to_string().contains("bad\nmodel"));
}

#[test]
fn model_search_matches_id_and_owner_case_insensitively() {
    let models = parse_model_list(
        br#"{"data":[
            {"id":"grok-4.5","owned_by":"xAI"},
            {"id":"gpt-5.6","owned_by":"OpenAI"},
            {"id":"grok-4.3-high","owned_by":"custom"}
        ]}"#,
    )
    .unwrap();

    assert_eq!(
        search_models(&models, "GROK")
            .into_iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["grok-4.5", "grok-4.3-high"]
    );
    assert_eq!(search_models(&models, "openai")[0].id, "gpt-5.6");
    assert_eq!(search_models(&models, "").len(), 3);
}

#[test]
fn injectable_model_filter_keeps_only_grok_ids_from_the_dynamic_catalog() {
    let models = parse_model_list(
        br#"{"data":[
            {"id":"grok-4.5","owned_by":"custom"},
            {"id":"gpt-5.6","owned_by":"openai"},
            {"id":"GROK-4.3-HIGH","owned_by":"custom"},
            {"id":"GROK-4.5","owned_by":"custom"},
            {"id":"grok-imagine-1","owned_by":"custom"},
            {"id":"not-grok-4","owned_by":"custom"}
        ]}"#,
    )
    .unwrap();

    assert_eq!(
        injectable_grok_models(&models)
            .into_iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        vec!["grok-4.5"]
    );
}

#[test]
fn model_fetch_uses_bearer_auth_and_never_returns_the_secret_in_errors() {
    let secret = "test-provider-secret";
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.1\r\n"));
        assert!(request.contains("Authorization: Bearer test-provider-secret\r\n"));
        let body = r#"{"data":[{"id":"grok-4.5","owned_by":"xai"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let models = fetch_model_list(&format!("http://{address}/v1"), secret).unwrap();
    server.join().unwrap();
    assert_eq!(models[0].id, "grok-4.5");

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let body = "test-provider-secret";
        write!(
            stream,
            "HTTP/1.1 401 Unauthorized\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let error = fetch_model_list(&format!("http://{address}/v1"), secret).unwrap_err();
    server.join().unwrap();
    assert!(error.to_string().contains("HTTP 401"));
    assert!(!error.to_string().contains(secret));
}
