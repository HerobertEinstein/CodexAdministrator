use std::fs;

use codex_administrator::{
    DEFAULT_GROK_ACTION_PATH, DEFAULT_GROK_BASE_URL, DiscoveredModel, LauncherSettings,
    RendererAddonSettings, load_launcher_settings, save_launcher_settings,
};
use tempfile::tempdir;

#[test]
fn launcher_settings_persist_only_non_secret_provider_state() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("launcher-settings.json");
    let settings = LauncherSettings {
        base_url: "https://example.com/v1".into(),
        selected_models: vec!["grok-4.5".into()],
        cached_models: vec![DiscoveredModel {
            id: "grok-4.5".into(),
            owned_by: Some("xai".into()),
        }],
        sync_native_auth: true,
        sync_native_sessions: true,
        renderer_addons: vec![RendererAddonSettings {
            id: "codex-dream-skin".into(),
            enabled: true,
            source_root: temp.path().join("Codex-Dream-Skin"),
        }],
        ..LauncherSettings::default()
    };

    save_launcher_settings(&path, &settings).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert!(!raw.to_ascii_lowercase().contains("api_key"));
    assert!(!raw.to_ascii_lowercase().contains("apikey"));
    assert_eq!(load_launcher_settings(&path).unwrap(), settings);
}

#[test]
fn launcher_settings_reject_invalid_urls_and_unknown_selected_models() {
    let mut settings = LauncherSettings {
        base_url: "http://example.com/v1".into(),
        ..LauncherSettings::default()
    };
    assert!(settings.validate().is_err());

    settings.base_url = "https://example.com/v1".into();
    settings.cached_models = vec![DiscoveredModel {
        id: "grok-4.5".into(),
        owned_by: None,
    }];
    settings.selected_models = vec!["grok-does-not-exist".into()];
    assert!(settings.validate().is_err());

    settings.cached_models.push(DiscoveredModel {
        id: "gpt-5.6".into(),
        owned_by: Some("openai".into()),
    });
    settings.selected_models = vec!["gpt-5.6".into()];
    assert!(settings.validate().is_err());

    settings.cached_models = vec![DiscoveredModel {
        id: "grok-imagine-1".into(),
        owned_by: Some("xai".into()),
    }];
    settings.selected_models = vec!["grok-imagine-1".into()];
    assert!(settings.validate().is_err());

    settings.cached_models = vec![DiscoveredModel {
        id: "grok-4.5".into(),
        owned_by: None,
    }];
    settings.selected_models = vec!["grok-4.5".into()];
    settings.action_path = "/chat/completions".into();
    settings.action_path_auto = false;
    assert!(settings.validate().is_err());

    settings.action_path = "/compatible/responses".into();
    assert!(settings.validate().is_ok());

    settings.renderer_addons = vec![RendererAddonSettings {
        id: "codex-dream-skin".into(),
        enabled: true,
        source_root: "relative-checkout".into(),
    }];
    assert!(settings.validate().is_err());

    settings.base_url.clear();
    settings.cached_models.clear();
    settings.selected_models.clear();
    assert!(settings.validate().is_err());
}

#[test]
fn missing_launcher_settings_use_safe_defaults() {
    let temp = tempdir().unwrap();
    let settings = load_launcher_settings(&temp.path().join("missing.json")).unwrap();

    assert_eq!(settings.base_url, DEFAULT_GROK_BASE_URL);
    assert_eq!(settings.action_path, DEFAULT_GROK_ACTION_PATH);
    assert!(settings.action_path_auto);
    assert!(settings.selected_models.is_empty());
    assert!(settings.cached_models.is_empty());
    assert!(settings.renderer_addons.is_empty());
    assert!(settings.sync_native_auth);
    assert!(!settings.sync_native_sessions);
}
