use std::fs;

use codex_administrator::{
    CODEX_PLUS_BOOTSTRAP_KEY, enable_codex_plus_bootstrap, install_bootstrap_atomically,
    remove_codex_plus_bootstrap,
};
use sha2::{Digest, Sha256};
use tempfile::tempdir;

#[test]
fn installs_the_generated_bootstrap_atomically_and_returns_its_digest() {
    let temp = tempdir().unwrap();
    let target = temp
        .path()
        .join("user_scripts")
        .join("codex-administrator-bootstrap.js");
    let script = b"window.__codexAdministrator = {};";

    let digest = install_bootstrap_atomically(&target, script).unwrap();

    assert_eq!(fs::read(&target).unwrap(), script);
    assert_eq!(digest, format!("{:x}", Sha256::digest(script)));
    assert_eq!(
        fs::read_dir(target.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .count(),
        1
    );
}

#[test]
fn replaces_an_old_bootstrap_without_leaving_a_partial_file() {
    let temp = tempdir().unwrap();
    let target = temp.path().join("codex-administrator-bootstrap.js");
    fs::write(&target, b"old").unwrap();

    install_bootstrap_atomically(&target, b"new").unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"new");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
}

#[test]
fn enables_only_our_codex_plus_script_and_preserves_unknown_configuration() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("user_scripts.json");
    fs::write(
        &config_path,
        r#"{
  "enabled": false,
  "scripts": {"user:other.js": false},
  "market": {"user:other.js": {"version": "7"}},
  "future_field": {"keep": true}
}"#,
    )
    .unwrap();

    enable_codex_plus_bootstrap(&config_path).unwrap();

    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(value["enabled"], true);
    assert_eq!(value["scripts"]["user:other.js"], false);
    assert_eq!(value["scripts"][CODEX_PLUS_BOOTSTRAP_KEY], true);
    assert_eq!(value["market"]["user:other.js"]["version"], "7");
    assert_eq!(value["future_field"]["keep"], true);
}

#[test]
fn creates_a_minimal_codex_plus_script_config_when_absent() {
    let temp = tempdir().unwrap();
    let config_path = temp.path().join("user_scripts.json");

    enable_codex_plus_bootstrap(&config_path).unwrap();

    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    assert_eq!(value["enabled"], true);
    assert_eq!(value["scripts"][CODEX_PLUS_BOOTSTRAP_KEY], true);
}

#[test]
fn removal_deletes_only_our_external_script_and_configuration_key() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");
    let root = appdata.join("Codex++");
    let scripts = root.join("user_scripts");
    fs::create_dir_all(&scripts).unwrap();
    fs::write(scripts.join("codex-administrator-bootstrap.js"), b"ours").unwrap();
    fs::write(scripts.join("other.js"), b"theirs").unwrap();
    fs::write(
        root.join("user_scripts.json"),
        format!(
            r#"{{"enabled":true,"scripts":{{"{}":true,"user:other.js":true}},"future_field":{{"keep":true}}}}"#,
            CODEX_PLUS_BOOTSTRAP_KEY
        ),
    )
    .unwrap();

    let receipt = remove_codex_plus_bootstrap(&appdata).unwrap();

    assert!(receipt.script_removed);
    assert!(receipt.config_updated);
    assert!(!scripts.join("codex-administrator-bootstrap.js").exists());
    assert_eq!(fs::read(scripts.join("other.js")).unwrap(), b"theirs");
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("user_scripts.json")).unwrap()).unwrap();
    assert!(config["scripts"].get(CODEX_PLUS_BOOTSTRAP_KEY).is_none());
    assert_eq!(config["scripts"]["user:other.js"], true);
    assert_eq!(config["future_field"]["keep"], true);
    assert_eq!(config["enabled"], true);
}

#[test]
fn removal_is_idempotent_when_our_files_are_absent() {
    let temp = tempdir().unwrap();
    let receipt = remove_codex_plus_bootstrap(temp.path()).unwrap();

    assert!(!receipt.script_removed);
    assert!(!receipt.config_updated);
}
