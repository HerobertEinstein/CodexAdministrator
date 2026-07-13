use std::{fs, process::Command};

use codex_administrator::CODEX_PLUS_BOOTSTRAP_KEY;
use tempfile::tempdir;

fn create_fake_codex_runtime(root: &std::path::Path) {
    let executable_name = if cfg!(windows) { "node.exe" } else { "node" };
    let node = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(executable_name))
        .find(|candidate| candidate.is_file())
        .expect("Node.js runtime is required for CLI contract tests");
    let script = root
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("bin")
        .join("codex.js");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::copy(node, root.join(executable_name)).unwrap();
    fs::write(&script, b"process.exit(0);\n").unwrap();
}

#[test]
fn top_level_help_exposes_launcher_and_diagnostics_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("serve"));
    assert!(stdout.contains("launch"));
    assert!(stdout.contains("launch-native"));
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("native ChatGPT/Codex model providers"));
}

#[test]
fn native_launch_help_accepts_only_an_environment_key_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["launch", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--base-url"));
    assert!(stdout.contains("--env-key"));
    assert!(stdout.contains("--config"));
    assert!(stdout.contains("--model-catalog"));
    assert!(stdout.contains("--workspace"));
    assert!(!stdout.to_ascii_lowercase().contains("--api-key"));
    assert!(!stdout.to_ascii_lowercase().contains("--secret"));
}

#[test]
fn native_launch_registers_the_provider_and_never_persists_the_secret() {
    let temp = tempdir().unwrap();
    let runtime_root = temp.path().join("runtime");
    create_fake_codex_runtime(&runtime_root);
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let config = temp.path().join("codex").join("config.toml");
    let secret = "test-secret-that-must-not-be-persisted";

    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args([
            "launch",
            "--base-url",
            "https://gateway.example/v1",
            "--env-key",
            "TEST_GROK_KEY",
            "--model",
            "grok-4",
            "--config",
        ])
        .arg(&config)
        .arg("--workspace")
        .arg(&workspace)
        .env("PATH", &runtime_root)
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
    assert!(rendered.contains("model_provider = \"grok_native\""));
    assert!(rendered.contains("model = \"grok-4\""));
    assert!(!rendered.contains(secret));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains(secret));
    assert!(stdout.contains("official_codex_app"));
}

#[test]
fn launch_native_restores_the_selection_saved_by_grok_launch() {
    let temp = tempdir().unwrap();
    let runtime_root = temp.path().join("runtime");
    create_fake_codex_runtime(&runtime_root);
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let config = temp.path().join("codex").join("config.toml");
    fs::create_dir_all(config.parent().unwrap()).unwrap();
    fs::write(
        &config,
        "model = \"gpt-native\"\nmodel_provider = \"openai\"\n",
    )
    .unwrap();

    let grok = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args([
            "launch",
            "--base-url",
            "https://gateway.example/v1",
            "--env-key",
            "TEST_GROK_KEY",
            "--model",
            "grok-4",
            "--config",
        ])
        .arg(&config)
        .arg("--workspace")
        .arg(&workspace)
        .env("PATH", &runtime_root)
        .env("CODEX_HOME", config.parent().unwrap())
        .env("TEST_GROK_KEY", "test-secret")
        .output()
        .unwrap();
    assert!(grok.status.success());

    let native = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["launch-native", "--config"])
        .arg(&config)
        .arg("--workspace")
        .arg(&workspace)
        .env("PATH", &runtime_root)
        .env("CODEX_HOME", config.parent().unwrap())
        .output()
        .unwrap();

    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let rendered = fs::read_to_string(&config).unwrap();
    assert!(rendered.contains("model = \"gpt-native\""));
    assert!(rendered.contains("model_provider = \"openai\""));
}

#[test]
fn serve_help_exposes_both_host_adapters_without_a_secret_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .args(["serve", "--help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--host"));
    assert!(stdout.contains("direct"));
    assert!(stdout.contains("codexplusplus"));
    assert!(!stdout.to_ascii_lowercase().contains("--capability"));
    assert!(!stdout.to_ascii_lowercase().contains("--token"));
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
    assert!(value.get("runtimes").is_some());
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
            "serve",
            "--host",
            "codexplusplus",
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
    assert_eq!(report["effective_mode"], "native_gpt_main");
    assert_eq!(report["injection_enabled"], false);
    assert_eq!(report["reason"], "unverified_host_identity");
    assert!(!scripts.join("codex-administrator-bootstrap.js").exists());
    assert_eq!(fs::read(scripts.join("other.js")).unwrap(), b"preserve");
}
