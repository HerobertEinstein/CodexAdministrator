use codex_administrator::{BootstrapConfig, render_bootstrap};

#[test]
fn renders_an_idempotent_namespaced_native_provider_bridge() {
    let script = render_bootstrap(&BootstrapConfig {
        port: 49_321,
        capability: "test-capability".into(),
    })
    .unwrap();

    assert!(script.contains("window.__codexAdministrator"));
    assert!(script.contains("grok_native_model"));
    assert!(script.contains("native_gpt_main"));
    assert!(script.contains("49321"));
    assert!(script.contains("test-capability"));
    assert!(script.contains("dispose"));
    assert!(script.contains("health"));
}

#[test]
fn bootstrap_does_not_create_a_second_cdp_bridge() {
    let script = render_bootstrap(&BootstrapConfig {
        port: 49_321,
        capability: "test-capability".into(),
    })
    .unwrap();

    for forbidden in [
        "Runtime.addBinding",
        "Page.addScriptToEvaluateOnNewDocument",
        "codexSessionDeleteV2",
        "__codexSessionDeleteBridge",
    ] {
        assert!(
            !script.contains(forbidden),
            "found forbidden CDP surface: {forbidden}"
        );
    }
}

#[test]
fn bootstrap_uses_the_authenticated_iframe_instead_of_cross_origin_fetch() {
    let script = render_bootstrap(&BootstrapConfig {
        port: 49_321,
        capability: "test-capability".into(),
    })
    .unwrap();

    assert!(!script.contains("fetch("));
    assert!(script.contains("postMessage"));
    assert!(script.contains("event.source"));
    assert!(script.contains("/ui/#capability="));
}

#[test]
fn bootstrap_dispose_removes_global_message_listeners_before_remount() {
    let script = render_bootstrap(&BootstrapConfig {
        port: 49_321,
        capability: "test-capability".into(),
    })
    .unwrap();

    assert!(script.contains("addEventListener(\"message\", handleMessage)"));
    assert!(script.contains("removeEventListener(\"message\", handleMessage)"));
}

#[test]
fn rejects_invalid_bootstrap_configuration() {
    assert!(
        render_bootstrap(&BootstrapConfig {
            port: 0,
            capability: "valid-capability".into(),
        })
        .is_err()
    );
    assert!(
        render_bootstrap(&BootstrapConfig {
            port: 49_321,
            capability: " ".into(),
        })
        .is_err()
    );
}
