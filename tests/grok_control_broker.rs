use std::sync::Mutex;

use anyhow::Result;
use codex_administrator::{
    ControlRequest, CredentialStore, DEFAULT_GROK_ACTION_PATH, DEFAULT_GROK_BASE_URL,
    DiscoveredModel, GrokControlBroker, LauncherSettings, RendererAddonSettings,
    bind_provider_credential, parse_control_requests, resolve_bound_provider_credential,
};
use serde_json::{Value, json};
use tempfile::tempdir;

const NONCE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[derive(Default)]
struct MemoryCredentialStore {
    secret: Mutex<Option<String>>,
}

impl CredentialStore for MemoryCredentialStore {
    fn read(&self) -> Result<Option<String>> {
        Ok(self.secret.lock().unwrap().clone())
    }

    fn write(&self, secret: &str) -> Result<()> {
        *self.secret.lock().unwrap() = Some(secret.into());
        Ok(())
    }

    fn delete(&self) -> Result<bool> {
        Ok(self.secret.lock().unwrap().take().is_some())
    }
}

struct FailingWriteCredentialStore;

impl CredentialStore for FailingWriteCredentialStore {
    fn read(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn write(&self, _secret: &str) -> Result<()> {
        anyhow::bail!("credential write failed")
    }

    fn delete(&self) -> Result<bool> {
        Ok(false)
    }
}

fn request(operation: &str, payload: Value) -> ControlRequest {
    let mut value = json!([{
        "version": 1,
        "id": "ca-1",
        "nonce": NONCE,
        "operation": operation,
        "payload": payload
    }]);
    parse_control_requests(&mut value, NONCE)
        .unwrap()
        .pop()
        .unwrap()
}

fn write_default_bound_credential(store: &MemoryCredentialStore, secret: &str) {
    let stored =
        bind_provider_credential(DEFAULT_GROK_BASE_URL, DEFAULT_GROK_ACTION_PATH, secret).unwrap();
    store.write(&stored).unwrap();
}

fn read_default_bound_credential(store: &MemoryCredentialStore) -> Option<String> {
    store.read().unwrap().and_then(|stored| {
        resolve_bound_provider_credential(DEFAULT_GROK_BASE_URL, DEFAULT_GROK_ACTION_PATH, &stored)
            .unwrap()
    })
}

#[test]
fn state_read_returns_non_secret_grok_picker_state() {
    let temp = tempdir().unwrap();
    let store = MemoryCredentialStore::default();
    let mut broker = GrokControlBroker::new(
        NONCE,
        LauncherSettings::default(),
        false,
        temp.path().join("settings.json"),
    )
    .unwrap();

    let outcome = broker.handle(
        request("state.read", json!({})),
        &store,
        |_, _| unreachable!(),
    );
    let response = outcome.response.into_value();

    assert_eq!(response["ok"], true);
    assert_eq!(
        response["result"]["model_picker"]["baseUrl"],
        DEFAULT_GROK_BASE_URL
    );
    assert_eq!(
        response["result"]["model_picker"]["actionPath"],
        DEFAULT_GROK_ACTION_PATH
    );
    assert_eq!(
        response["result"]["model_picker"]["credentialPresent"],
        false
    );
    assert_eq!(response["result"]["model_picker"]["syncNativeSkills"], true);
    assert!(response.to_string().find("credential_value").is_none());
}

#[test]
fn addon_only_apply_persists_external_checkout_settings_and_requests_restart() {
    let temp = tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    let store = MemoryCredentialStore::default();
    let mut broker = GrokControlBroker::new(
        NONCE,
        LauncherSettings::default(),
        false,
        settings_path.clone(),
    )
    .unwrap();
    let addon_root = temp.path().join("Codex-Dream-Skin");

    let applied = broker.handle(
        request(
            "config.apply",
            json!({
                "base_url": DEFAULT_GROK_BASE_URL,
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": true,
                "selected_models": [],
                "renderer_addons": [{
                    "id": "codex-dream-skin",
                    "enabled": true,
                    "source_root": addon_root
                }],
                "sync_native_auth": true,
                "sync_native_sessions": false,
                "sync_native_skills": false
            }),
        ),
        &store,
        |_, _| unreachable!(),
    );

    assert!(applied.restart_required);
    assert_eq!(applied.response.into_value()["ok"], true);
    let saved: LauncherSettings =
        serde_json::from_slice(&std::fs::read(settings_path).unwrap()).unwrap();
    assert_eq!(
        saved.renderer_addons,
        [RendererAddonSettings {
            id: "codex-dream-skin".into(),
            enabled: true,
            source_root: addon_root,
        }]
    );
    assert!(saved.selected_models.is_empty());
    assert!(!saved.sync_native_skills);
}

#[test]
fn rejected_apply_preserves_the_last_valid_in_memory_settings() {
    let temp = tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    let store = MemoryCredentialStore::default();
    let initial_settings = LauncherSettings::default();
    let expected_sync_native_sessions = initial_settings.sync_native_sessions;
    let mut broker =
        GrokControlBroker::new(NONCE, initial_settings, false, settings_path.clone()).unwrap();

    let rejected = broker.handle(
        request(
            "config.apply",
            json!({
                "base_url": "https://example.com/v1",
                "action_path": "/custom-responses",
                "action_path_auto": false,
                "selected_models": [],
                "renderer_addons": [{
                    "id": "codex-dream-skin",
                    "enabled": true,
                    "source_root": "relative-checkout"
                }],
                "sync_native_auth": false,
                "sync_native_sessions": true
            }),
        ),
        &store,
        |_, _| unreachable!(),
    );
    assert_eq!(rejected.response.into_value()["ok"], false);
    assert!(!settings_path.exists());

    let state = broker.handle(
        request("state.read", json!({})),
        &store,
        |_, _| unreachable!(),
    );
    let state = state.response.into_value();
    assert_eq!(
        state["result"]["model_picker"]["baseUrl"],
        DEFAULT_GROK_BASE_URL
    );
    assert_eq!(
        state["result"]["model_picker"]["actionPath"],
        DEFAULT_GROK_ACTION_PATH
    );
    assert_eq!(state["result"]["model_picker"]["syncNativeAuth"], true);
    assert_eq!(
        state["result"]["model_picker"]["syncNativeSessions"],
        expected_sync_native_sessions
    );
    assert_eq!(state["result"]["model_picker"]["rendererAddons"], json!([]));
}

#[test]
fn credential_write_failure_does_not_persist_settings_or_consume_the_pending_key() {
    let temp = tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    let discovery_store = MemoryCredentialStore::default();
    let initial_settings = LauncherSettings::default();
    let expected_sync_native_sessions = initial_settings.sync_native_sessions;
    let mut broker =
        GrokControlBroker::new(NONCE, initial_settings, false, settings_path.clone()).unwrap();

    let discovered = broker.handle(
        request(
            "models.discover",
            json!({
                "base_url": DEFAULT_GROK_BASE_URL,
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": true,
                "credential": "pending-provider-key"
            }),
        ),
        &discovery_store,
        |_, _| {
            Ok(vec![DiscoveredModel {
                id: "grok-4.5".into(),
                owned_by: Some("xai".into()),
            }])
        },
    );
    assert_eq!(discovered.response.into_value()["ok"], true);

    let payload = json!({
        "base_url": DEFAULT_GROK_BASE_URL,
        "action_path": DEFAULT_GROK_ACTION_PATH,
        "action_path_auto": true,
        "selected_models": ["grok-4.5"],
        "sync_native_auth": false,
        "sync_native_sessions": true
    });
    let rejected = broker.handle(
        request("config.apply", payload.clone()),
        &FailingWriteCredentialStore,
        |_, _| unreachable!(),
    );
    assert_eq!(rejected.response.into_value()["ok"], false);
    assert!(!settings_path.exists());

    let state = broker.handle(
        request("state.read", json!({})),
        &discovery_store,
        |_, _| unreachable!(),
    );
    let state = state.response.into_value();
    assert_eq!(state["result"]["model_picker"]["syncNativeAuth"], true);
    assert_eq!(
        state["result"]["model_picker"]["syncNativeSessions"],
        expected_sync_native_sessions
    );

    let working_store = MemoryCredentialStore::default();
    let applied = broker.handle(
        request("config.apply", payload),
        &working_store,
        |_, _| unreachable!(),
    );
    assert_eq!(applied.response.into_value()["ok"], true);
    assert_eq!(
        read_default_bound_credential(&working_store).as_deref(),
        Some("pending-provider-key")
    );
}

#[test]
fn discovery_keeps_only_grok_and_apply_persists_no_secret_in_settings() {
    let temp = tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    let store = MemoryCredentialStore::default();
    let mut broker = GrokControlBroker::new(
        NONCE,
        LauncherSettings::default(),
        false,
        settings_path.clone(),
    )
    .unwrap();

    let discovered = broker.handle(
        request(
            "models.discover",
            json!({
                "base_url": DEFAULT_GROK_BASE_URL,
                "action_path": "/wrong-but-auto",
                "action_path_auto": true,
                "credential": "transient-provider-key"
            }),
        ),
        &store,
        |base_url, credential| {
            assert_eq!(base_url, DEFAULT_GROK_BASE_URL);
            assert_eq!(credential, "transient-provider-key");
            Ok(vec![
                DiscoveredModel {
                    id: "grok-4.5".into(),
                    owned_by: Some("xai".into()),
                },
                DiscoveredModel {
                    id: "gpt-5.6".into(),
                    owned_by: Some("openai".into()),
                },
                DiscoveredModel {
                    id: "grok-imagine-1".into(),
                    owned_by: Some("xai".into()),
                },
            ])
        },
    );
    let response = discovered.response.into_value();
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["models"].as_array().unwrap().len(), 1);
    assert_eq!(response["result"]["models"][0]["id"], "grok-4.5");
    assert_eq!(
        response["result"]["model_picker"]["actionPath"],
        DEFAULT_GROK_ACTION_PATH
    );
    assert!(store.read().unwrap().is_none());

    let applied = broker.handle(
        request(
            "config.apply",
            json!({
                "base_url": DEFAULT_GROK_BASE_URL,
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": true,
                "selected_models": ["grok-4.5"],
                "sync_native_auth": true,
                "sync_native_sessions": false
            }),
        ),
        &store,
        |_, _| unreachable!(),
    );
    assert!(applied.restart_required);
    assert_eq!(
        read_default_bound_credential(&store).as_deref(),
        Some("transient-provider-key")
    );
    let raw = std::fs::read_to_string(settings_path).unwrap();
    assert!(!raw.contains("transient-provider-key"));
    assert!(!raw.to_ascii_lowercase().contains("api_key"));
}

#[test]
fn stored_credentials_are_bound_to_the_saved_provider_endpoint() {
    let temp = tempdir().unwrap();
    let store = MemoryCredentialStore::default();
    write_default_bound_credential(&store, "stored-provider-key");
    let mut broker = GrokControlBroker::new(
        NONCE,
        LauncherSettings::default(),
        true,
        temp.path().join("settings.json"),
    )
    .unwrap();
    let mut discovery_called = false;

    let rejected = broker.handle(
        request(
            "models.discover",
            json!({
                "base_url": "https://other.example/v1",
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": true,
                "credential": ""
            }),
        ),
        &store,
        |_, _| {
            discovery_called = true;
            Ok(Vec::new())
        },
    );

    let response = rejected.response.into_value();
    assert_eq!(response["ok"], false);
    assert!(!discovery_called);
    assert_eq!(
        read_default_bound_credential(&store).as_deref(),
        Some("stored-provider-key")
    );
}

#[test]
fn persisted_credential_is_not_reused_after_offline_endpoint_replacement() {
    let temp = tempdir().unwrap();
    let store = MemoryCredentialStore::default();
    write_default_bound_credential(&store, "stored-provider-key");
    let settings = LauncherSettings {
        base_url: "https://other.example/v1".into(),
        ..LauncherSettings::default()
    };
    let mut broker =
        GrokControlBroker::new(NONCE, settings, true, temp.path().join("settings.json")).unwrap();
    let mut discovery_called = false;

    let rejected = broker.handle(
        request(
            "models.discover",
            json!({
                "base_url": "https://other.example/v1",
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": true,
                "credential": ""
            }),
        ),
        &store,
        |_, _| {
            discovery_called = true;
            Ok(Vec::new())
        },
    );

    assert_eq!(rejected.response.into_value()["ok"], false);
    assert!(!discovery_called);
}

#[test]
fn a_fresh_credential_cannot_be_applied_to_a_different_endpoint() {
    let temp = tempdir().unwrap();
    let settings_path = temp.path().join("settings.json");
    let store = MemoryCredentialStore::default();
    let mut broker = GrokControlBroker::new(
        NONCE,
        LauncherSettings::default(),
        false,
        settings_path.clone(),
    )
    .unwrap();

    let discovered = broker.handle(
        request(
            "models.discover",
            json!({
                "base_url": DEFAULT_GROK_BASE_URL,
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": true,
                "credential": "fresh-provider-key"
            }),
        ),
        &store,
        |_, _| {
            Ok(vec![DiscoveredModel {
                id: "grok-4.5".into(),
                owned_by: Some("xai".into()),
            }])
        },
    );
    assert_eq!(discovered.response.into_value()["ok"], true);

    let rejected = broker.handle(
        request(
            "config.apply",
            json!({
                "base_url": "https://other.example/v1",
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": true,
                "selected_models": ["grok-4.5"],
                "sync_native_auth": true,
                "sync_native_sessions": false
            }),
        ),
        &store,
        |_, _| unreachable!(),
    );

    assert_eq!(rejected.response.into_value()["ok"], false);
    assert!(store.read().unwrap().is_none());
    assert!(!settings_path.exists());
}

#[test]
fn apply_rejects_non_grok_selection_and_clear_removes_the_saved_credential() {
    let temp = tempdir().unwrap();
    let store = MemoryCredentialStore::default();
    write_default_bound_credential(&store, "stored-provider-key");
    let settings = LauncherSettings {
        cached_models: vec![DiscoveredModel {
            id: "grok-4.5".into(),
            owned_by: None,
        }],
        ..LauncherSettings::default()
    };
    let mut broker =
        GrokControlBroker::new(NONCE, settings, true, temp.path().join("settings.json")).unwrap();

    let rejected = broker.handle(
        request(
            "config.apply",
            json!({
                "base_url": DEFAULT_GROK_BASE_URL,
                "action_path": DEFAULT_GROK_ACTION_PATH,
                "action_path_auto": false,
                "selected_models": ["gpt-5.6"],
                "sync_native_auth": true,
                "sync_native_sessions": false
            }),
        ),
        &store,
        |_, _| unreachable!(),
    );
    assert_eq!(rejected.response.into_value()["ok"], false);

    let cleared = broker.handle(
        request("credential.clear", json!({})),
        &store,
        |_, _| unreachable!(),
    );
    assert!(cleared.restart_required);
    assert_eq!(cleared.response.into_value()["ok"], true);
    assert!(store.read().unwrap().is_none());
}
