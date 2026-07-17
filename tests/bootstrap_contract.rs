use codex_administrator::{
    BootstrapConfig, InjectedModelDescriptor, ModelPickerConfig, RendererAddonCatalogEntry,
    RendererAddonReport, RendererAddonSettings, RendererAddonState, render_bootstrap,
};

fn config() -> BootstrapConfig {
    BootstrapConfig {
        models: vec![InjectedModelDescriptor::grok("grok-4.5")],
        model_picker: ModelPickerConfig::default(),
    }
}

#[test]
fn grok_4_5_exposes_the_official_reasoning_efforts() {
    let model = InjectedModelDescriptor::grok("grok-4.5");
    let efforts = model
        .supported_reasoning_efforts
        .iter()
        .map(|effort| effort.reasoning_effort.as_str())
        .collect::<Vec<_>>();

    assert_eq!(efforts, ["low", "medium", "high"]);
    assert_eq!(model.default_reasoning_effort, "high");
    assert_eq!(model.input_modalities, ["text", "image"]);
}

#[test]
fn fixed_effort_grok_aliases_expose_only_the_effort_encoded_in_the_model_id() {
    let high = InjectedModelDescriptor::grok("grok-4.3-high");
    assert_eq!(high.default_reasoning_effort, "high");
    assert_eq!(high.supported_reasoning_efforts.len(), 1);
    assert_eq!(high.supported_reasoning_efforts[0].reasoning_effort, "high");

    let xhigh = InjectedModelDescriptor::grok("grok-4.20-multi-agent-xhigh");
    assert_eq!(xhigh.default_reasoning_effort, "xhigh");
    assert_eq!(
        xhigh.supported_reasoning_efforts[0].reasoning_effort,
        "xhigh"
    );
}

#[test]
fn grok_descriptor_rejects_non_grok_model_ids() {
    let config = BootstrapConfig {
        models: vec![InjectedModelDescriptor::grok("gpt-5.6")],
        model_picker: ModelPickerConfig::default(),
    };
    assert!(render_bootstrap(&config).is_err());
}

#[test]
fn grok_descriptor_rejects_unreviewed_grok_capability_profiles() {
    for model in [
        "grok-imagine-1",
        "grok-4.3-fast",
        "grok-4.20-0309-non-reasoning",
        "GROK-4.5",
        "GROK-4.3-HIGH",
    ] {
        let config = BootstrapConfig {
            models: vec![InjectedModelDescriptor::grok(model)],
            model_picker: ModelPickerConfig::default(),
        };
        assert!(
            render_bootstrap(&config).is_err(),
            "unreviewed model {model} was accepted"
        );
    }
}

#[test]
fn renders_an_idempotent_namespaced_native_provider_bridge() {
    let script = render_bootstrap(&config()).unwrap();

    assert!(script.contains("__codexAdministratorRendererApiDiscovery"));
    assert!(script.contains("window.__codexAdministrator"));
    assert!(script.contains("__codexAdministratorModelInjectionCore"));
    assert!(script.contains("__codexAdministratorModelPickerMount"));
    assert!(script.contains("data-codex-intelligence-trigger"));
    assert!(script.contains("data-codex-administrator-model-manager"));
    assert!(script.contains("https://ai.hebox.net/v1"));
    assert!(script.contains("\"actionPath\":\"/responses\""));
    assert!(script.contains("\"credentialPresent\":"));
    assert!(script.contains("\"syncNativeSkills\":true"));
    assert!(!script.contains("credentialValue"));
    assert!(!script.contains("apiKeyValue"));
    assert!(script.contains("grok_native"));
    assert!(script.contains("grok-4.5"));
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

    assert!(!script.contains("createElement(\"iframe\")"));
    assert!(!script.contains("createRoot("));
    assert!(!script.contains("innerHTML"));
    assert!(!script.contains("/ui/"));
}

#[test]
fn model_picker_control_nonce_is_ephemeral_and_well_formed() {
    let first = ModelPickerConfig::default().control_nonce;
    let second = ModelPickerConfig::default().control_nonce;

    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(first, second);
}

#[test]
fn bootstrap_dispose_removes_global_message_listeners_before_remount() {
    let script = render_bootstrap(&config()).unwrap();

    assert!(script.contains("addEventListener(\"message\", handleMessage, true)"));
    assert!(script.contains("removeEventListener(\"message\", handleMessage, true)"));
    assert!(script.contains("setInterval(maintainModelPickerMount, 500)"));
    assert!(script.contains("clearInterval(modelPickerRetryTimer)"));
}

#[test]
fn management_only_bootstrap_mounts_the_native_model_manager_without_injected_models() {
    let script = render_bootstrap(&BootstrapConfig {
        models: Vec::new(),
        model_picker: ModelPickerConfig::default(),
    })
    .unwrap();

    assert!(script.contains("__codexAdministratorModelPickerMount"));
    assert!(script.contains("data-codex-administrator-model-manager"));
    assert!(script.contains("\"models\":[]"));
}

#[test]
fn bootstrap_exposes_only_non_secret_reviewed_renderer_addon_state() {
    let mut config = config();
    config.model_picker.renderer_addons = vec![RendererAddonSettings {
        id: "codex-dream-skin".into(),
        enabled: true,
        source_root: r"C:\Injectors\Codex-Dream-Skin".into(),
    }];
    config.model_picker.renderer_addon_catalog = vec![RendererAddonCatalogEntry {
        id: "codex-dream-skin".into(),
        display_name: "Codex Dream Skin".into(),
        project_revision: "reviewed-commit".into(),
    }];
    config.model_picker.renderer_addon_reports = vec![RendererAddonReport {
        id: "codex-dream-skin".into(),
        state: RendererAddonState::Enabled,
        project_revision: Some("reviewed-commit".into()),
        reason: None,
        blocked_by: None,
    }];

    let script = render_bootstrap(&config).unwrap();

    assert!(script.contains("\"rendererAddons\":"));
    assert!(script.contains("\"rendererAddonCatalog\":"));
    assert!(script.contains("Codex Dream Skin"));
    assert!(script.contains("codex-dream-skin"));
    assert!(script.contains("reviewed-commit"));
    assert!(script.contains("\"hostAdapter\":\"direct\""));
    assert!(!script.contains("provider-secret"));
}
