use std::fs;

use codex_administrator::{GrokNativeProviderConfig, install_grok_native_provider};
use tempfile::tempdir;

fn provider() -> GrokNativeProviderConfig {
    GrokNativeProviderConfig {
        base_url: "https://gateway.example/v1".into(),
        env_key: "GROK_NATIVE_API_KEY".into(),
        supports_websockets: false,
    }
}

#[test]
fn installs_only_the_responses_provider_without_changing_native_selection() {
    let temp = tempdir().unwrap();
    let config = temp.path().join("config.toml");
    fs::write(
        &config,
        r#"model = "gpt-native"
model_provider = "openai"
model_catalog_json = "official-models.json"
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
    assert!(rendered.contains("model_provider = \"openai\""));
    assert!(rendered.contains("model_catalog_json = \"official-models.json\""));
    assert!(rendered.contains("[model_providers.existing]"));
}

#[test]
fn provider_registration_is_idempotent_and_creates_no_selection_sidecar() {
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
    assert!(!temp.path().join("codex-administrator").exists());
}

#[test]
fn rejects_insecure_remote_endpoints_queries_and_invalid_environment_keys() {
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
