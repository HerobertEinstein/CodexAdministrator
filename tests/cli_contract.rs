use std::process::Command;

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
