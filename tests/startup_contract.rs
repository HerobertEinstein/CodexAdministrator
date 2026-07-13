use std::fs;

use codex_administrator::{
    BootstrapConfig, CODEX_PLUS_BOOTSTRAP_KEY, generate_capability, prepare_codex_plus_host,
};
use tempfile::tempdir;

#[test]
fn generated_launch_capabilities_are_unique_256_bit_hex_values() {
    let first = generate_capability();
    let second = generate_capability();

    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_ne!(first, second);
}

#[test]
fn prepares_the_optional_codex_plus_host_without_touching_its_binaries() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");
    let receipt = prepare_codex_plus_host(
        &appdata,
        &BootstrapConfig {
            port: 49_321,
            capability: "0123456789abcdef0123456789abcdef".into(),
        },
    )
    .unwrap();

    assert_eq!(
        receipt.bootstrap_path,
        appdata
            .join("Codex++")
            .join("user_scripts")
            .join("codex-administrator-bootstrap.js")
    );
    assert_eq!(receipt.sha256.len(), 64);
    let script = fs::read_to_string(&receipt.bootstrap_path).unwrap();
    assert!(script.contains("window.__codexAdministrator"));
    assert!(script.contains("49321"));

    let config: serde_json::Value = serde_json::from_slice(
        &fs::read(appdata.join("Codex++").join("user_scripts.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(config["scripts"][CODEX_PLUS_BOOTSTRAP_KEY], true);
    assert_eq!(fs::read_dir(appdata.join("Codex++")).unwrap().count(), 2);
}
