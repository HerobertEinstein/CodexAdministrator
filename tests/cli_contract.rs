use std::{fs, process::Command};

use codex_administrator::CODEX_PLUS_BOOTSTRAP_KEY;
use tempfile::tempdir;

#[test]
fn top_level_help_exposes_only_current_provider_and_injection_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("configure-provider"));
    assert!(stdout.contains("inject"));
    assert!(stdout.contains("doctor"));
    assert!(!stdout.contains("serve"));
    assert!(stdout.contains("Grok model-list injection"));
}

#[test]
fn provider_configuration_help_accepts_only_an_environment_key_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["configure-provider", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--base-url"));
    assert!(stdout.contains("--env-key"));
    assert!(stdout.contains("--config"));
    assert!(!stdout.contains("--model-catalog"));
    assert!(!stdout.contains("--workspace"));
    assert!(!stdout.to_ascii_lowercase().contains("--api-key"));
    assert!(!stdout.to_ascii_lowercase().contains("--secret"));
}

#[test]
fn provider_configuration_never_changes_native_models_or_persists_the_secret() {
    let temp = tempdir().unwrap();
    let config = temp.path().join("codex").join("config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(
        &config,
        "model = \"gpt-native\"\nmodel_provider = \"openai\"\nmodel_catalog_json = \"official-models.json\"\n",
    )
    .unwrap();
    let secret = "test-secret-that-must-not-be-persisted";

    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args([
            "configure-provider",
            "--base-url",
            "https://gateway.example/v1",
            "--env-key",
            "TEST_GROK_KEY",
            "--config",
        ])
        .arg(&config)
        .env("CODEX_HOME", config.parent().unwrap())
        .env("TEST_GROK_KEY", secret)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rendered = fs::read_to_string(&config).unwrap();
    assert!(rendered.contains("[model_providers.grok_native]"));
    assert!(rendered.contains("env_key = \"TEST_GROK_KEY\""));
    assert!(rendered.contains("model_provider = \"openai\""));
    assert!(rendered.contains("model = \"gpt-native\""));
    assert!(rendered.contains("model_catalog_json = \"official-models.json\""));
    assert!(!rendered.contains(secret));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains(secret));
    assert!(stdout.contains("provider_configured"));
}

#[test]
fn injection_help_exposes_both_host_adapters_without_a_secret_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["inject", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--host"));
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("direct"));
    assert!(stdout.contains("codexplusplus"));
    assert!(!stdout.to_ascii_lowercase().contains("--capability"));
    assert!(!stdout.to_ascii_lowercase().contains("--token"));
}

#[test]
fn direct_injection_is_explicitly_disabled_until_the_isolated_launcher_exists() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args([
            "inject",
            "--host",
            "direct",
            "--model",
            "grok-4",
            "--no-launch",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("isolated launcher is not implemented"));
}

#[test]
fn doctor_emits_machine_readable_output_without_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["product"], "Codex Administrator");
    assert!(value.get("adapters").is_some());
    assert_eq!(
        value["adapters"]["direct"]["reason"],
        "isolated_launcher_not_implemented"
    );
    let rendered = value.to_string().to_ascii_lowercase();
    assert!(!rendered.contains("capability"));
    assert!(!rendered.contains("api_key"));
}

#[test]
fn unknown_codex_plus_binary_falls_back_natively_and_removes_stale_injection() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");
    let root = appdata.join("Codex++");
    let scripts = root.join("user_scripts");
    fs::create_dir_all(&scripts).unwrap();
    fs::write(scripts.join("codex-administrator-bootstrap.js"), b"stale").unwrap();
    fs::write(scripts.join("other.js"), b"preserve").unwrap();
    fs::write(
        root.join("user_scripts.json"),
        format!(
            r#"{{"enabled":true,"scripts":{{"{}":true,"user:other.js":true}}}}"#,
            CODEX_PLUS_BOOTSTRAP_KEY
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args([
            "inject",
            "--host",
            "codexplusplus",
            "--model",
            "grok-4",
            "--codex-plus-path",
            env!("CARGO_BIN_EXE_codex-administrator"),
            "--appdata",
        ])
        .arg(&appdata)
        .arg("--no-launch")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "native_fallback");
    assert_eq!(report["injection_enabled"], false);
    assert_eq!(report["reason"], "unverified_host_identity");
    assert!(!scripts.join("codex-administrator-bootstrap.js").exists());
    assert_eq!(fs::read(scripts.join("other.js")).unwrap(), b"preserve");
}
