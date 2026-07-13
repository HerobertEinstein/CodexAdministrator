use std::{fs, process::Command};

use codex_administrator::CODEX_PLUS_BOOTSTRAP_KEY;
use tempfile::tempdir;

#[test]
fn top_level_help_exposes_launcher_and_diagnostics_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_codex-administrator"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("serve"));
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("dual-main-agent"));
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
