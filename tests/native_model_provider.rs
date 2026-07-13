use std::fs;

use std::path::PathBuf;

use codex_administrator::{
    GROK_NATIVE_PROVIDER_ID, GrokNativeProviderConfig, NativeProviderCapabilities,
    NativeProviderCapabilityManifest, RuntimeLaunchSpec, build_codex_native_app_launch,
    install_grok_native_provider, install_grok_native_provider_for_model,
    restore_native_model_selection, validate_codex_model_catalog,
};
use tempfile::tempdir;

fn provider() -> GrokNativeProviderConfig {
    GrokNativeProviderConfig {
        base_url: "https://gateway.example/v1".into(),
        env_key: "GROK_NATIVE_API_KEY".into(),
        supports_websockets: false,
    }
}

#[test]
fn installs_a_responses_provider_without_persisting_a_secret_or_changing_defaults() {
    let temp = tempdir().unwrap();
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        r#"model = "gpt-native"
approval_policy = "on-request"

[model_providers.existing]
name = "Existing"
base_url = "https://existing.example/v1"
wire_api = "responses"
"#,
    )
    .unwrap();

    let receipt = install_grok_native_provider(&config, &provider()).unwrap();

    assert!(receipt.updated);
    let rendered = fs::read_to_string(&config).unwrap();
    assert!(rendered.contains("[model_providers.grok_native]"));
    assert!(rendered.contains("name = \"Grok in native ChatGPT/Codex\""));
    assert!(rendered.contains("base_url = \"https://gateway.example/v1\""));
    assert!(rendered.contains("env_key = \"GROK_NATIVE_API_KEY\""));
    assert!(rendered.contains("wire_api = \"responses\""));
    assert!(rendered.contains("requires_openai_auth = false"));
    assert!(rendered.contains("model = \"gpt-native\""));
    assert!(rendered.contains("[model_providers.existing]"));
    assert!(!rendered.contains("sk-"));
}

#[test]
fn provider_installation_is_idempotent_and_preserves_future_fields() {
    let temp = tempdir().unwrap();
    let config = temp.path().join("config.toml");
    fs::write(&config, "future_field = { keep = true }\n").unwrap();

    assert!(
        install_grok_native_provider(&config, &provider())
            .unwrap()
            .updated
    );
    assert!(
        !install_grok_native_provider(&config, &provider())
            .unwrap()
            .updated
    );
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("future_field = { keep = true }")
    );
}

#[test]
fn rejects_insecure_remote_endpoints_and_invalid_environment_keys() {
    let mut invalid = provider();
    invalid.base_url = "http://gateway.example/v1".into();
    assert!(invalid.validate().is_err());

    invalid.base_url = "http://127.0.0.1:18790/v1".into();
    assert!(invalid.validate().is_ok());

    invalid.base_url = "https://gateway.example/v1?tenant=other".into();
    assert!(invalid.validate().is_err());

    invalid.env_key = "actual-secret-value".into();
    assert!(invalid.validate().is_err());
}

#[test]
fn builds_an_official_codex_app_launch_without_a_secret_or_shell() {
    let runtime = RuntimeLaunchSpec::codex(PathBuf::from(r"C:\Tools\codex.exe"));

    let launch =
        build_codex_native_app_launch(&runtime, &PathBuf::from(r"D:\Work\project")).unwrap();

    assert_eq!(launch.executable, PathBuf::from(r"C:\Tools\codex.exe"));
    assert_eq!(launch.args, ["app", r"D:\Work\project"]);
    assert!(!launch.use_shell);
    assert!(!launch.args.iter().any(|value| value == "-c"));
}

#[test]
fn preserves_the_official_npm_codex_entrypoint_for_native_app_launch() {
    let script = PathBuf::from(r"C:\npm\node_modules\@openai\codex\bin\codex.js");
    let runtime = RuntimeLaunchSpec::codex_node(PathBuf::from(r"C:\npm\node.exe"), script.clone());

    let launch =
        build_codex_native_app_launch(&runtime, &PathBuf::from(r"D:\Work\project")).unwrap();

    assert_eq!(launch.executable, PathBuf::from(r"C:\npm\node.exe"));
    assert_eq!(
        launch.args.first(),
        Some(&script.to_string_lossy().into_owned())
    );
    assert_eq!(launch.args[1], "app");
    assert!(!launch.use_shell);
}

#[test]
fn validates_and_persists_a_reviewed_model_catalog_for_the_official_app() {
    let temp = tempdir().unwrap();
    let catalog = temp.path().join("grok-models.json");
    fs::write(
        &catalog,
        r#"{"models":[{"slug":"grok-4","display_name":"Grok 4","description":null,"supported_reasoning_levels":[],"shell_type":"shell_command","visibility":"list","supported_in_api":true,"priority":0,"availability_nux":null,"upgrade":null,"base_instructions":"","supports_reasoning_summaries":false,"support_verbosity":false,"default_verbosity":null,"apply_patch_tool_type":null,"truncation_policy":{"mode":"tokens","limit":10000},"supports_parallel_tool_calls":false,"experimental_supported_tools":[]}]}"#,
    )
    .unwrap();
    validate_codex_model_catalog(&catalog, "grok-4").unwrap();
    assert!(validate_codex_model_catalog(&catalog, "grok-unknown").is_err());
    let incomplete_catalog = temp.path().join("incomplete-grok-models.json");
    fs::write(
        &incomplete_catalog,
        r#"{"models":[{"slug":"grok-4","display_name":"Grok 4"}]}"#,
    )
    .unwrap();
    assert!(validate_codex_model_catalog(&incomplete_catalog, "grok-4").is_err());

    let config = temp.path().join("config.toml");
    install_grok_native_provider_for_model(&config, &provider(), "grok-4", Some(&catalog)).unwrap();

    let rendered = fs::read_to_string(config).unwrap();
    assert!(rendered.contains("model_provider = \"grok_native\""));
    assert!(rendered.contains("model = \"grok-4\""));
    assert!(rendered.contains("model_catalog_json"));
    assert!(rendered.contains("grok-models.json"));
}

#[test]
fn model_selection_rejects_invalid_ids_without_touching_the_config() {
    let temp = tempdir().unwrap();
    let config = temp.path().join("config.toml");

    assert!(install_grok_native_provider_for_model(&config, &provider(), " ", None).is_err());
    assert!(
        install_grok_native_provider_for_model(&config, &provider(), " grok-4 ", None).is_err()
    );
    assert!(install_grok_native_provider_for_model(&config, &provider(), "grok\n4", None).is_err());
    assert!(!config.exists());
}

#[test]
fn restores_the_previous_native_selection_after_repeated_grok_launches() {
    let temp = tempdir().unwrap();
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        "model = \"gpt-native\"\nmodel_provider = \"openai\"\n",
    )
    .unwrap();

    install_grok_native_provider_for_model(&config, &provider(), "grok-4", None).unwrap();
    install_grok_native_provider_for_model(&config, &provider(), "grok-4-fast", None).unwrap();

    let selected = fs::read_to_string(&config).unwrap();
    assert!(selected.contains("model = \"grok-4-fast\""));
    assert!(selected.contains("model_provider = \"grok_native\""));

    let receipt = restore_native_model_selection(&config).unwrap();
    assert!(receipt.updated);
    let restored = fs::read_to_string(&config).unwrap();
    assert!(restored.contains("model = \"gpt-native\""));
    assert!(restored.contains("model_provider = \"openai\""));
}

#[test]
fn restore_fails_closed_after_a_manual_selection_change() {
    let temp = tempdir().unwrap();
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        "model = \"gpt-native\"\nmodel_provider = \"openai\"\n",
    )
    .unwrap();
    install_grok_native_provider_for_model(&config, &provider(), "grok-4", None).unwrap();

    let changed = fs::read_to_string(&config)
        .unwrap()
        .replace("model = \"grok-4\"", "model = \"user-selected\"");
    fs::write(&config, changed).unwrap();

    assert!(restore_native_model_selection(&config).is_err());
    assert!(
        fs::read_to_string(&config)
            .unwrap()
            .contains("model = \"user-selected\"")
    );
}

#[test]
fn unknown_provider_capabilities_default_to_disabled() {
    let capabilities = NativeProviderCapabilities::default();

    assert!(!capabilities.responses);
    assert!(!capabilities.streaming);
    assert!(!capabilities.tool_calls);
    assert!(!capabilities.image_input);
    assert!(!capabilities.file_input);
    assert!(!capabilities.native_codex_agent_ready());
    assert!(!capabilities.multimodal_ready());
}

#[test]
fn capability_manifests_require_exact_provider_model_and_evidence_identity() {
    let manifest = NativeProviderCapabilityManifest::from_json(
        format!(
            r#"{{
  "schema_version": 1,
  "provider_id": "{GROK_NATIVE_PROVIDER_ID}",
  "models": ["grok-4"],
  "capabilities": {{
    "responses": true,
    "streaming": true,
    "tool_calls": true,
    "parallel_tool_calls": false,
    "image_input": true,
    "file_input": false,
    "structured_outputs": false,
    "reasoning_summaries": false,
    "websockets": false
  }},
  "evidence_sha256": "{}"
}}"#,
            "a".repeat(64)
        )
        .as_bytes(),
    )
    .unwrap();

    assert!(manifest.supports_model("grok-4"));
    assert!(!manifest.supports_model("grok-unknown"));
    assert!(manifest.capabilities.native_codex_agent_ready());
    assert!(!manifest.capabilities.multimodal_ready());
    assert!(NativeProviderCapabilityManifest::from_json(br#"{}"#).is_err());
    assert!(
        NativeProviderCapabilityManifest::from_json(
            format!(
                r#"{{"schema_version":1,"provider_id":"grok_native","models":[" grok-4 "],"capabilities":{{}},"evidence_sha256":"{}"}}"#,
                "b".repeat(64)
            )
            .as_bytes()
        )
        .is_err()
    );
}
