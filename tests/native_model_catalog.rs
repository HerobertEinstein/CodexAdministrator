use std::{env, fs, path::PathBuf};

use codex_administrator::{InjectedModelDescriptor, install_grok_native_model_catalog};
use serde_json::{Value, json};
use tempfile::tempdir;

fn native_model(slug: &str, priority: i32) -> Value {
    json!({
        "slug": slug,
        "display_name": slug,
        "description": "Official native model",
        "default_reasoning_level": "high",
        "supported_reasoning_levels": [
            {"effort": "low", "description": "Low"},
            {"effort": "high", "description": "High"}
        ],
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": priority,
        "additional_speed_tiers": [],
        "service_tiers": [],
        "default_service_tier": null,
        "availability_nux": null,
        "upgrade": null,
        "base_instructions": "OFFICIAL_NATIVE_INSTRUCTIONS",
        "model_messages": null,
        "include_skills_usage_instructions": false,
        "supports_reasoning_summaries": false,
        "default_reasoning_summary": "auto",
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": null,
        "web_search_tool_type": "text",
        "truncation_policy": {"mode": "bytes", "limit": 10000},
        "supports_parallel_tool_calls": true,
        "supports_image_detail_original": false,
        "context_window": 272000,
        "max_context_window": 272000,
        "auto_compact_token_limit": null,
        "comp_hash": null,
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text"],
        "supports_search_tool": false,
        "use_responses_lite": false,
        "auto_review_model_override": null,
        "tool_mode": null,
        "multi_agent_version": null,
        "future_required_shape": {"mode": "official-default"}
    })
}

fn write_fake_official_binary(path: &std::path::Path, models: Vec<Value>) {
    let catalog = serde_json::to_vec_pretty(&json!({"models": models})).unwrap();
    let mut binary = b"signed-official-prefix\0".to_vec();
    binary.extend(catalog);
    binary.extend(b"\0signed-official-suffix");
    fs::write(path, binary).unwrap();
}

fn write_fake_windows_official_binary(path: &std::path::Path, models: Vec<Value>) {
    let catalog = serde_json::to_string_pretty(&json!({"models": models}))
        .unwrap()
        .replace('\n', "\r\n");
    let mut binary = b"signed-official-prefix\0".to_vec();
    binary.extend(catalog.as_bytes());
    binary.extend(b"\0signed-official-suffix");
    fs::write(path, binary).unwrap();
}

#[test]
fn installs_a_hidden_grok_overlay_on_the_complete_official_catalog() {
    let temp = tempdir().unwrap();
    let official_binary = temp.path().join("codex.exe");
    let catalog_path = temp.path().join("grok-native-model-catalog.json");
    let config_path = temp.path().join("config.toml");
    let native_models = vec![
        native_model("gpt-native-a", 0),
        native_model("gpt-native-b", 1),
    ];
    write_fake_official_binary(&official_binary, native_models.clone());
    fs::write(&config_path, "model = \"gpt-native-a\"\n").unwrap();

    install_grok_native_model_catalog(
        &official_binary,
        &catalog_path,
        &config_path,
        &[InjectedModelDescriptor::grok("grok-4.5")],
    )
    .unwrap();

    let installed: Value = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
    let models = installed["models"].as_array().unwrap();
    assert_eq!(&models[..2], native_models.as_slice());
    assert_eq!(models[2]["slug"], "grok-4.5");
    assert_eq!(models[2]["visibility"], "none");
    assert_eq!(
        models[2]["availability_nux"]["message"],
        "codex-administrator:grok-native-catalog-v1"
    );
    assert_eq!(models[2]["default_reasoning_level"], "high");
    assert_eq!(models[2]["supports_reasoning_summaries"], false);
    assert_eq!(models[2]["default_reasoning_summary"], "none");
    assert_eq!(models[2]["supports_parallel_tool_calls"], false);
    assert_eq!(models[2]["supports_search_tool"], false);
    assert_eq!(models[2]["context_window"], 32_768);
    assert_eq!(models[2]["max_context_window"], 32_768);
    assert_eq!(models[2]["input_modalities"], json!(["text", "image"]));
    assert_eq!(
        models[2]["future_required_shape"],
        json!({"mode": "official-default"})
    );
    assert!(
        models[2]["base_instructions"]
            .as_str()
            .unwrap()
            .contains("OFFICIAL_NATIVE_INSTRUCTIONS")
    );

    let config = fs::read_to_string(&config_path).unwrap();
    assert!(config.contains("model = \"gpt-native-a\""));
    assert!(config.contains("model_catalog_json"));
    assert!(config.contains(catalog_path.to_str().unwrap()));
}

#[test]
fn rejects_a_grok_slug_that_is_already_official() {
    let temp = tempdir().unwrap();
    let official_binary = temp.path().join("codex.exe");
    let catalog_path = temp.path().join("grok-native-model-catalog.json");
    let config_path = temp.path().join("config.toml");
    write_fake_official_binary(&official_binary, vec![native_model("grok-4.5", 0)]);

    let error = install_grok_native_model_catalog(
        &official_binary,
        &catalog_path,
        &config_path,
        &[InjectedModelDescriptor::grok("grok-4.5")],
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("collides with the official model catalog")
    );
}

#[test]
fn fills_newer_required_reasoning_summary_capability_on_older_official_entries() {
    let temp = tempdir().unwrap();
    let official_binary = temp.path().join("codex.exe");
    let catalog_path = temp.path().join("grok-native-model-catalog.json");
    let config_path = temp.path().join("config.toml");
    let mut no_summary = native_model("gpt-native-none", 0);
    no_summary
        .as_object_mut()
        .unwrap()
        .remove("supports_reasoning_summaries");
    no_summary["default_reasoning_summary"] = json!("none");
    let mut auto_summary = native_model("gpt-native-auto", 1);
    auto_summary
        .as_object_mut()
        .unwrap()
        .remove("supports_reasoning_summaries");
    auto_summary["default_reasoning_summary"] = json!("auto");
    write_fake_official_binary(&official_binary, vec![no_summary, auto_summary]);

    install_grok_native_model_catalog(
        &official_binary,
        &catalog_path,
        &config_path,
        &[InjectedModelDescriptor::grok("grok-4.5")],
    )
    .unwrap();

    let installed: Value = serde_json::from_slice(&fs::read(catalog_path).unwrap()).unwrap();
    assert_eq!(
        installed["models"][0]["supports_reasoning_summaries"],
        false
    );
    assert_eq!(installed["models"][1]["supports_reasoning_summaries"], true);
}

#[test]
fn extracts_the_crlf_catalog_embedded_by_the_windows_official_binary() {
    let temp = tempdir().unwrap();
    let official_binary = temp.path().join("codex.exe");
    let catalog_path = temp.path().join("grok-native-model-catalog.json");
    let config_path = temp.path().join("config.toml");
    write_fake_windows_official_binary(&official_binary, vec![native_model("gpt-native", 0)]);

    install_grok_native_model_catalog(
        &official_binary,
        &catalog_path,
        &config_path,
        &[InjectedModelDescriptor::grok("grok-4.5")],
    )
    .unwrap();

    let installed: Value = serde_json::from_slice(&fs::read(&catalog_path).unwrap()).unwrap();
    assert_eq!(installed["models"][0]["slug"], "gpt-native");
    assert_eq!(installed["models"][1]["slug"], "grok-4.5");
}

#[test]
#[ignore = "rewrites an explicitly supplied live isolated catalog and config"]
fn live_isolated_catalog_inherits_the_current_official_schema() {
    let official = required_path("CODEX_ADMINISTRATOR_TEST_OFFICIAL_CODEX");
    let catalog = required_path("CODEX_ADMINISTRATOR_TEST_MODEL_CATALOG");
    let config = required_path("CODEX_ADMINISTRATOR_TEST_ISOLATED_CONFIG");
    let models = env::var("CODEX_ADMINISTRATOR_TEST_GROK_MODELS")
        .expect("set CODEX_ADMINISTRATOR_TEST_GROK_MODELS")
        .split(',')
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(InjectedModelDescriptor::grok)
        .collect::<Vec<_>>();

    install_grok_native_model_catalog(&official, &catalog, &config, &models).unwrap();

    let installed: Value = serde_json::from_slice(&fs::read(catalog).unwrap()).unwrap();
    assert!(installed["models"].as_array().unwrap().iter().any(|model| {
        model.get("slug").and_then(Value::as_str) == Some(models[0].model.as_str())
            && model.get("supports_reasoning_summaries") == Some(&json!(false))
    }));
}

fn required_path(name: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {name}"))
}
