use std::{fs, path::Path};

use codex_administrator::{
    HostAdapterKind, RendererAddonPolicy, RendererAddonSettings, RendererAddonState,
    prepare_renderer_addons,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_fixture(root: &Path) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let template =
        br#"((css, image) => ({ css, image }))(__DREAM_CSS_JSON__, __DREAM_ART_JSON__)"#.to_vec();
    let css = b".reviewed-skin { color: coral; }".to_vec();
    let image = vec![1_u8, 2, 3, 4, 5];
    let assets = root.join("windows").join("assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("renderer-inject.js"), &template).unwrap();
    fs::write(assets.join("dream-skin.css"), &css).unwrap();
    fs::write(assets.join("dream-reference.png"), &image).unwrap();
    (template, css, image)
}

fn policy(template: &[u8], css: &[u8], image: &[u8]) -> RendererAddonPolicy {
    RendererAddonPolicy::from_json(
        serde_json::to_vec(&json!({
            "schema_version": 2,
            "addons": [{
                "id": "reviewed-skin",
                "display_name": "Reviewed Skin",
                "project_revision": "reviewed-commit",
                "distribution": "external_checkout_only",
                "load_order": 200,
                "host_adapters": ["direct", "codexplusplus"],
                "exclusive_slots": ["theme"],
                "conflicts_with": [],
                "entrypoint": {
                    "path": "windows/assets/renderer-inject.js",
                    "sha256": sha256(template),
                    "max_bytes": 65536
                },
                "substitutions": [{
                    "placeholder": "__DREAM_CSS_JSON__",
                    "encoding": "json_utf8",
                    "asset": {
                        "path": "windows/assets/dream-skin.css",
                        "sha256": sha256(css),
                        "max_bytes": 65536
                    }
                }, {
                    "placeholder": "__DREAM_ART_JSON__",
                    "encoding": "data_url_base64",
                    "media_type": "image/png",
                    "asset": {
                        "path": "windows/assets/dream-reference.png",
                        "sha256": sha256(image),
                        "max_bytes": 1048576
                    }
                }],
                "lifecycle": {
                    "state_key": "__REVIEWED_SKIN_STATE__",
                    "dispose_method": "cleanup"
                }
            }]
        }))
        .unwrap()
        .as_slice(),
    )
    .unwrap()
}

#[test]
fn reviewed_external_renderer_addon_is_composed_after_the_core_bootstrap() {
    let temp = tempdir().unwrap();
    let (template, css, image) = write_fixture(temp.path());
    let bundle = prepare_renderer_addons(
        &[RendererAddonSettings {
            id: "reviewed-skin".into(),
            enabled: true,
            source_root: temp.path().to_path_buf(),
        }],
        &policy(&template, &css, &image),
        HostAdapterKind::Direct,
    );

    assert_eq!(bundle.reports()[0].state, RendererAddonState::Enabled);
    assert_eq!(
        bundle.reports()[0].project_revision.as_deref(),
        Some("reviewed-commit")
    );
    let rendered = bundle.compose("CORE_BOOTSTRAP");
    assert!(rendered.starts_with("CORE_BOOTSTRAP"));
    assert!(rendered.contains("__codexAdministratorRendererAddons"));
    assert!(rendered.contains(".reviewed-skin { color: coral; }"));
    assert!(rendered.contains("data:image/png;base64,AQIDBAU="));
    assert!(!rendered.contains(&temp.path().display().to_string()));
}

#[test]
fn changed_external_assets_disable_only_that_addon() {
    let temp = tempdir().unwrap();
    let (template, css, image) = write_fixture(temp.path());
    fs::write(
        temp.path()
            .join("windows")
            .join("assets")
            .join("dream-skin.css"),
        b"changed upstream css",
    )
    .unwrap();
    let bundle = prepare_renderer_addons(
        &[RendererAddonSettings {
            id: "reviewed-skin".into(),
            enabled: true,
            source_root: temp.path().to_path_buf(),
        }],
        &policy(&template, &css, &image),
        HostAdapterKind::Direct,
    );

    assert_eq!(bundle.reports()[0].state, RendererAddonState::Disabled);
    assert_eq!(
        bundle.reports()[0].reason.as_deref(),
        Some("source_identity_mismatch")
    );
    let rendered = bundle.compose("CORE_BOOTSTRAP");
    assert!(rendered.starts_with("CORE_BOOTSTRAP"));
    assert!(rendered.contains("__codexAdministratorRendererAddons"));
    assert!(!rendered.contains("changed upstream css"));
}

#[test]
fn addon_policy_rejects_embedded_distribution_and_parent_paths() {
    let invalid_distribution = json!({
        "schema_version": 2,
        "addons": [{
            "id": "unsafe",
            "display_name": "Unsafe",
            "project_revision": "commit",
            "distribution": "embedded",
            "load_order": 10,
            "host_adapters": ["direct"],
            "exclusive_slots": [],
            "conflicts_with": [],
            "entrypoint": { "path": "renderer.js", "sha256": "a".repeat(64), "max_bytes": 1 },
            "substitutions": [],
            "lifecycle": { "state_key": "__UNSAFE_STATE__", "dispose_method": "cleanup" }
        }]
    });
    assert!(
        RendererAddonPolicy::from_json(&serde_json::to_vec(&invalid_distribution).unwrap())
            .is_err()
    );

    let parent_path = json!({
        "schema_version": 2,
        "addons": [{
            "id": "unsafe",
            "display_name": "Unsafe",
            "project_revision": "commit",
            "distribution": "external_checkout_only",
            "load_order": 10,
            "host_adapters": ["direct"],
            "exclusive_slots": [],
            "conflicts_with": [],
            "entrypoint": { "path": "../renderer.js", "sha256": "a".repeat(64), "max_bytes": 1 },
            "substitutions": [],
            "lifecycle": { "state_key": "__UNSAFE_STATE__", "dispose_method": "cleanup" }
        }]
    });
    assert!(RendererAddonPolicy::from_json(&serde_json::to_vec(&parent_path).unwrap()).is_err());
}

#[test]
fn compatible_addons_compose_in_manifest_order_and_conflicts_disable_only_the_loser() {
    let temp = tempdir().unwrap();
    let first = b"globalThis.__FIRST_STATE__ = { cleanup() {} }; 'FIRST_ADDON';";
    let second = b"globalThis.__SECOND_STATE__ = { cleanup() {} }; 'SECOND_ADDON';";
    fs::write(temp.path().join("first.js"), first).unwrap();
    fs::write(temp.path().join("second.js"), second).unwrap();
    let policy = RendererAddonPolicy::from_json(
        &serde_json::to_vec(&json!({
            "schema_version": 2,
            "addons": [{
                "id": "second",
                "display_name": "Second",
                "project_revision": "second-revision",
                "distribution": "external_checkout_only",
                "load_order": 20,
                "host_adapters": ["direct", "codexplusplus"],
                "exclusive_slots": ["theme"],
                "conflicts_with": [],
                "entrypoint": { "path": "second.js", "sha256": sha256(second), "max_bytes": 1024 },
                "substitutions": [],
                "lifecycle": { "state_key": "__SECOND_STATE__", "dispose_method": "cleanup" }
            }, {
                "id": "first",
                "display_name": "First",
                "project_revision": "first-revision",
                "distribution": "external_checkout_only",
                "load_order": 10,
                "host_adapters": ["direct", "codexplusplus"],
                "exclusive_slots": [],
                "conflicts_with": [],
                "entrypoint": { "path": "first.js", "sha256": sha256(first), "max_bytes": 1024 },
                "substitutions": [],
                "lifecycle": { "state_key": "__FIRST_STATE__", "dispose_method": "cleanup" }
            }, {
                "id": "blocked-theme",
                "display_name": "Blocked Theme",
                "project_revision": "blocked-revision",
                "distribution": "external_checkout_only",
                "load_order": 30,
                "host_adapters": ["direct"],
                "exclusive_slots": ["theme"],
                "conflicts_with": [],
                "entrypoint": { "path": "first.js", "sha256": sha256(first), "max_bytes": 1024 },
                "substitutions": [],
                "lifecycle": { "state_key": "__BLOCKED_STATE__", "dispose_method": "cleanup" }
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let settings = ["blocked-theme", "second", "first"].map(|id| RendererAddonSettings {
        id: id.into(),
        enabled: true,
        source_root: temp.path().to_path_buf(),
    });

    let bundle = prepare_renderer_addons(&settings, &policy, HostAdapterKind::Direct);
    let rendered = bundle.compose("CORE");

    assert!(rendered.find("FIRST_ADDON").unwrap() < rendered.find("SECOND_ADDON").unwrap());
    assert!(!rendered.contains("__BLOCKED_STATE__"));
    let blocked = bundle
        .reports()
        .iter()
        .find(|report| report.id == "blocked-theme")
        .unwrap();
    assert_eq!(blocked.state, RendererAddonState::Disabled);
    assert_eq!(blocked.reason.as_deref(), Some("exclusive_slot_conflict"));
    assert_eq!(blocked.blocked_by.as_deref(), Some("second"));
}

#[test]
fn addon_catalog_is_non_secret_and_host_scoped() {
    let temp = tempdir().unwrap();
    let (template, css, image) = write_fixture(temp.path());
    let policy = policy(&template, &css, &image);

    let catalog = policy.catalog(HostAdapterKind::CodexPlusPlus);

    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].id, "reviewed-skin");
    assert_eq!(catalog[0].display_name, "Reviewed Skin");
    assert_eq!(catalog[0].project_revision, "reviewed-commit");
    assert!(!format!("{catalog:?}").contains(&temp.path().display().to_string()));
}
