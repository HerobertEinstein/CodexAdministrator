use codex_administrator::{bind_provider_credential, resolve_bound_provider_credential};

#[test]
fn stored_provider_credentials_are_bound_to_the_verified_endpoint() {
    let stored = bind_provider_credential(
        "https://gateway.example/v1",
        "/responses",
        "test-provider-secret",
    )
    .unwrap();

    assert!(!stored.contains("https://gateway.example/v1"));
    assert_eq!(
        resolve_bound_provider_credential("https://gateway.example/v1", "/responses", &stored,)
            .unwrap()
            .as_deref(),
        Some("test-provider-secret")
    );
    assert_eq!(
        resolve_bound_provider_credential("https://other.example/v1", "/responses", &stored,)
            .unwrap(),
        None
    );
    assert_eq!(
        resolve_bound_provider_credential(
            "https://gateway.example/v1",
            "/compatible/responses",
            &stored,
        )
        .unwrap(),
        None
    );
}

#[test]
fn legacy_unbound_credentials_are_never_reused() {
    assert_eq!(
        resolve_bound_provider_credential(
            "https://gateway.example/v1",
            "/responses",
            "legacy-unbound-secret",
        )
        .unwrap(),
        None
    );
}
