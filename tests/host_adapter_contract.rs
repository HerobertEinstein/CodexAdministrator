use std::path::Path;

use codex_administrator::{HostAdapterKind, InjectionStrategy, codex_plus_bootstrap_path};

#[test]
fn direct_and_codex_plus_plus_are_distinct_public_host_adapters() {
    assert_eq!(
        serde_json::to_string(&HostAdapterKind::Direct).unwrap(),
        r#""direct""#
    );
    assert_eq!(
        serde_json::to_string(&HostAdapterKind::CodexPlusPlus).unwrap(),
        r#""codexplusplus""#
    );
}

#[test]
fn each_host_adapter_has_exactly_one_injection_owner() {
    assert_eq!(
        HostAdapterKind::Direct.injection_strategy(),
        InjectionStrategy::ProjectOwnedCdp
    );
    assert_eq!(
        HostAdapterKind::CodexPlusPlus.injection_strategy(),
        InjectionStrategy::ExternalUserScript
    );
}

#[test]
fn codex_plus_plus_adapter_uses_only_its_external_user_script_data_surface() {
    let appdata = Path::new(r"C:\Users\Example\AppData\Roaming");
    let path = codex_plus_bootstrap_path(appdata);

    assert_eq!(
        path,
        appdata
            .join("Codex++")
            .join("user_scripts")
            .join("codex-administrator-bootstrap.js")
    );
    assert!(!path.to_string_lossy().contains("app.asar"));
}

#[test]
fn host_launcher_rejects_a_missing_executable_before_requesting_elevation() {
    let missing = Path::new(r"C:\definitely-missing\codex-plus-plus.exe");

    assert!(codex_administrator::launch_host_executable(missing).is_err());
}
