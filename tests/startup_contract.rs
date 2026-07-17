use std::fs;

use codex_administrator::{
    BootstrapConfig, CODEX_PLUS_BOOTSTRAP_KEY, CompatibilityPolicy, HostAdapterKind, HostIdentity,
    InjectedModelDescriptor, ModelPickerConfig, codex_plus_launch_allowed, prepare_codex_plus_host,
    prepare_codex_plus_host_guarded, prepare_codex_plus_host_script,
};
use tempfile::tempdir;

fn bootstrap_config() -> BootstrapConfig {
    BootstrapConfig {
        models: vec![InjectedModelDescriptor::grok("grok-4.5")],
        model_picker: ModelPickerConfig::default(),
    }
}

#[test]
fn prepares_the_optional_codex_plus_host_without_touching_its_binaries() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");
    let receipt = prepare_codex_plus_host(&appdata, &bootstrap_config()).unwrap();

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
    assert!(script.contains("grok-4.5"));
    assert!(script.contains("grok_native"));
    assert!(!script.contains("capability"));

    let config: serde_json::Value = serde_json::from_slice(
        &fs::read(appdata.join("Codex++").join("user_scripts.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(config["scripts"][CODEX_PLUS_BOOTSTRAP_KEY], true);
    assert_eq!(fs::read_dir(appdata.join("Codex++")).unwrap().count(), 2);
}

#[test]
fn codex_plus_host_can_install_an_already_composed_renderer_bundle() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");

    let receipt =
        prepare_codex_plus_host_script(&appdata, "CORE_BOOTSTRAP\nOPTIONAL_REVIEWED_SKIN").unwrap();

    assert_eq!(
        fs::read_to_string(receipt.bootstrap_path).unwrap(),
        "CORE_BOOTSTRAP\nOPTIONAL_REVIEWED_SKIN"
    );
}

#[test]
fn guarded_startup_prepares_injection_only_for_an_exact_approved_identity() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");
    let sha256 = "a".repeat(64);
    let policy = CompatibilityPolicy::default()
        .allow_host_sha256(HostAdapterKind::CodexPlusPlus, &sha256)
        .unwrap();

    let outcome = prepare_codex_plus_host_guarded(
        &appdata,
        &bootstrap_config(),
        Some(&HostIdentity {
            adapter: HostAdapterKind::CodexPlusPlus,
            sha256,
        }),
        &policy,
    );

    assert!(outcome.decision.injection_enabled());
    assert!(outcome.bootstrap.is_some());
    assert!(outcome.isolation_error.is_none());
    assert!(codex_plus_launch_allowed(false, &outcome));
    assert!(!codex_plus_launch_allowed(true, &outcome));
}

#[test]
fn guarded_startup_removes_stale_injection_for_an_updated_unknown_host() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");
    let root = appdata.join("Codex++");
    let scripts = root.join("user_scripts");
    fs::create_dir_all(&scripts).unwrap();
    fs::write(scripts.join("codex-administrator-bootstrap.js"), b"stale").unwrap();
    fs::write(
        root.join("user_scripts.json"),
        format!(
            r#"{{"enabled":true,"scripts":{{"{}":true,"user:other.js":true}}}}"#,
            CODEX_PLUS_BOOTSTRAP_KEY
        ),
    )
    .unwrap();

    let outcome = prepare_codex_plus_host_guarded(
        &appdata,
        &bootstrap_config(),
        Some(&HostIdentity {
            adapter: HostAdapterKind::CodexPlusPlus,
            sha256: "f".repeat(64),
        }),
        &CompatibilityPolicy::default(),
    );

    assert!(!outcome.decision.injection_enabled());
    assert!(outcome.bootstrap.is_none());
    assert!(!codex_plus_launch_allowed(false, &outcome));
    assert!(!scripts.join("codex-administrator-bootstrap.js").exists());
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("user_scripts.json")).unwrap()).unwrap();
    assert!(config["scripts"].get(CODEX_PLUS_BOOTSTRAP_KEY).is_none());
    assert_eq!(config["scripts"]["user:other.js"], true);
}

#[test]
fn guarded_startup_fails_closed_when_bootstrap_preparation_fails() {
    let temp = tempdir().unwrap();
    let appdata = temp.path().join("AppData").join("Roaming");
    let root = appdata.join("Codex++");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("user_scripts.json"), b"not-json").unwrap();
    let sha256 = "a".repeat(64);
    let policy = CompatibilityPolicy::default()
        .allow_host_sha256(HostAdapterKind::CodexPlusPlus, &sha256)
        .unwrap();

    let outcome = prepare_codex_plus_host_guarded(
        &appdata,
        &bootstrap_config(),
        Some(&HostIdentity {
            adapter: HostAdapterKind::CodexPlusPlus,
            sha256,
        }),
        &policy,
    );

    assert!(!outcome.decision.injection_enabled());
    assert!(outcome.bootstrap.is_none());
    assert!(outcome.isolation_error.is_some());
    assert!(
        !root
            .join("user_scripts")
            .join("codex-administrator-bootstrap.js")
            .exists()
    );
}
