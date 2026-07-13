use codex_administrator::{BootstrapConfig, InjectedModelDescriptor, render_bootstrap};

fn config() -> BootstrapConfig {
    BootstrapConfig {
        models: vec![InjectedModelDescriptor::grok("grok-4")],
    }
}

#[test]
fn renders_an_idempotent_namespaced_native_provider_bridge() {
    let script = render_bootstrap(&config()).unwrap();

    assert!(script.contains("window.__codexAdministrator"));
    assert!(script.contains("__codexAdministratorModelInjectionCore"));
    assert!(script.contains("grok_native"));
    assert!(script.contains("grok-4"));
    assert!(script.contains("model/list"));
    assert!(script.contains("thread/start"));
    assert!(script.contains("dispose"));
    assert!(script.contains("health"));
    assert!(!script.contains("model_provider ="));
}

#[test]
fn bootstrap_does_not_create_a_second_cdp_bridge() {
    let script = render_bootstrap(&config()).unwrap();

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
fn bootstrap_has_no_replacement_interface() {
    let script = render_bootstrap(&config()).unwrap();

    assert!(!script.contains("fetch("));
    assert!(!script.contains("createElement(\"iframe\")"));
    assert!(!script.contains("postMessage"));
    assert!(!script.contains("/ui/"));
}

#[test]
fn bootstrap_dispose_removes_global_message_listeners_before_remount() {
    let script = render_bootstrap(&config()).unwrap();

    assert!(script.contains("addEventListener(\"message\", handleMessage, true)"));
    assert!(script.contains("removeEventListener(\"message\", handleMessage, true)"));
}

#[test]
fn rejects_invalid_bootstrap_configuration() {
    assert!(render_bootstrap(&BootstrapConfig { models: Vec::new() }).is_err());
}
