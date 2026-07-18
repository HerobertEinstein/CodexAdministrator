use std::fs;

use codex_administrator::{
    CompatibilityDecision, CompatibilityManifest, CompatibilityPolicy, HostAdapterKind,
    HostIdentity,
};
use tempfile::tempdir;

fn policy() -> CompatibilityPolicy {
    CompatibilityPolicy::default()
        .allow_host_sha256(HostAdapterKind::Direct, &"a".repeat(64))
        .unwrap()
        .allow_isolated_codex_plus_host_sha256(&"b".repeat(64))
        .unwrap()
}

#[test]
fn codex_plus_hash_without_isolated_owner_contract_is_rejected() {
    let result = CompatibilityPolicy::default()
        .allow_host_sha256(HostAdapterKind::CodexPlusPlus, &"b".repeat(64));

    assert!(result.is_err());
}

#[test]
fn isolated_codex_plus_owner_contract_may_enable_only_the_exact_identity() {
    let manifest = CompatibilityManifest::from_json(
        format!(
            r#"{{"schema_version":2,"hosts":[{{"adapter":"codexplusplus","sha256":"{}","project_version":"{}","bootstrap_version":2,"evidence_sha256":"{}","composition_contract":"isolated_codex_plus_owner_v1"}}]}}"#,
            "d".repeat(64),
            env!("CARGO_PKG_VERSION"),
            "e".repeat(64),
        )
        .as_bytes(),
    )
    .unwrap();
    let policy = manifest.into_policy().unwrap();

    assert!(
        policy
            .evaluate(HostAdapterKind::CodexPlusPlus, Some(&"d".repeat(64)))
            .injection_enabled()
    );
    assert!(
        !policy
            .evaluate(HostAdapterKind::CodexPlusPlus, Some(&"f".repeat(64)))
            .injection_enabled()
    );
}

#[test]
fn codex_plus_manifest_entry_requires_the_isolated_owner_contract() {
    let manifest = CompatibilityManifest::from_json(
        format!(
            r#"{{"schema_version":2,"hosts":[{{"adapter":"codexplusplus","sha256":"{}","project_version":"{}","bootstrap_version":2,"evidence_sha256":"{}"}}]}}"#,
            "d".repeat(64),
            env!("CARGO_PKG_VERSION"),
            "e".repeat(64),
        )
        .as_bytes(),
    )
    .unwrap();
    let error = manifest.into_policy().unwrap_err();

    assert!(error.to_string().contains("isolated_codex_plus_owner_v1"));
}

#[test]
fn verified_host_versions_may_enable_the_requested_model_selection_bridge() {
    let decision = policy().evaluate(HostAdapterKind::CodexPlusPlus, Some(&"b".repeat(64)));

    assert_eq!(decision, CompatibilityDecision::Enabled);
}

#[test]
fn unknown_host_versions_fail_closed_to_native_gpt() {
    let decision = policy().evaluate(HostAdapterKind::CodexPlusPlus, Some(&"c".repeat(64)));

    assert_eq!(
        decision,
        CompatibilityDecision::NativeOnly {
            reason: "unverified_host_identity".into(),
        }
    );
}

#[test]
fn missing_version_evidence_never_enables_injection() {
    let decision = policy().evaluate(HostAdapterKind::Direct, None);

    assert!(!decision.injection_enabled());
}

#[test]
fn policy_rejects_blank_version_entries() {
    assert!(
        CompatibilityPolicy::default()
            .allow_host_sha256(HostAdapterKind::Direct, "  ")
            .is_err()
    );
}

#[test]
fn host_identity_is_derived_from_the_executable_contents() {
    let temp = tempdir().unwrap();
    let executable = temp.path().join("CodexPlusPlus.exe");
    fs::write(&executable, b"first-build").unwrap();
    let first = HostIdentity::from_executable(HostAdapterKind::CodexPlusPlus, &executable).unwrap();

    fs::write(&executable, b"second-build").unwrap();
    assert!(!first.matches_executable(&executable).unwrap());
    let second =
        HostIdentity::from_executable(HostAdapterKind::CodexPlusPlus, &executable).unwrap();

    assert_eq!(first.sha256.len(), 64);
    assert_ne!(first.sha256, second.sha256);
    assert_eq!(first.adapter, HostAdapterKind::CodexPlusPlus);
}

#[test]
fn compatibility_manifest_accepts_only_exact_binary_identities() {
    let manifest = CompatibilityManifest::from_json(
        format!(
            r#"{{"schema_version":2,"hosts":[{{"adapter":"codexplusplus","sha256":"{}","project_version":"{}","bootstrap_version":2,"evidence_sha256":"{}","composition_contract":"isolated_codex_plus_owner_v1"}}]}}"#,
            "d".repeat(64),
            env!("CARGO_PKG_VERSION"),
            "e".repeat(64),
        )
        .as_bytes(),
    )
    .unwrap();
    let policy = manifest.into_policy().unwrap();

    assert!(
        policy
            .evaluate(HostAdapterKind::CodexPlusPlus, Some(&"d".repeat(64)),)
            .injection_enabled()
    );
    assert!(
        !policy
            .evaluate(HostAdapterKind::CodexPlusPlus, Some(&"e".repeat(64)),)
            .injection_enabled()
    );
}

#[test]
fn compatibility_manifest_rejects_malformed_or_future_schema_data() {
    assert!(CompatibilityManifest::from_json(br#"{"schema_version":3,"hosts":[]}"#).is_err());
    assert!(CompatibilityManifest::from_json(
        format!(
            r#"{{"schema_version":2,"hosts":[{{"adapter":"direct","sha256":"{}","project_version":"{}","bootstrap_version":2}}]}}"#,
            "a".repeat(64),
            env!("CARGO_PKG_VERSION"),
        )
        .as_bytes()
    )
    .is_err());
    assert!(CompatibilityManifest::from_json(
        format!(
            r#"{{"schema_version":2,"hosts":[{{"adapter":"direct","sha256":"{}","project_version":"old-release","bootstrap_version":2,"evidence_sha256":"{}"}}]}}"#,
            "a".repeat(64),
            "b".repeat(64),
        )
        .as_bytes()
    )
    .unwrap()
    .into_policy()
    .is_err());
    assert!(
        CompatibilityPolicy::default()
            .allow_host_sha256(HostAdapterKind::Direct, &"z".repeat(64))
            .is_err()
    );
}

#[test]
fn shipped_manifest_is_embedded_and_parseable() {
    CompatibilityManifest::shipped()
        .unwrap()
        .into_policy()
        .unwrap();
}
