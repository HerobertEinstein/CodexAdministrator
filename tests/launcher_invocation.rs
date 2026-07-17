use std::path::Path;

use codex_administrator::{
    DiscoveredModel, LauncherSettings, PROVIDER_RUNTIME_ENV_KEY, RendererAddonSettings,
    SupervisorGeneration, build_direct_launcher_arguments, environment_variable_is_sensitive,
    launcher_output_is_ready, sanitize_launcher_diagnostic, spawn_direct_launcher,
};

fn settings() -> LauncherSettings {
    LauncherSettings {
        base_url: "https://example.com/v1".into(),
        selected_models: vec!["grok-4.5".into(), "grok-4.3-high".into()],
        cached_models: vec![
            DiscoveredModel {
                id: "grok-4.5".into(),
                owned_by: Some("xai".into()),
            },
            DiscoveredModel {
                id: "grok-4.3-high".into(),
                owned_by: Some("custom".into()),
            },
        ],
        sync_native_auth: true,
        sync_native_sessions: true,
        sync_native_goals: true,
        ..LauncherSettings::default()
    }
}

#[test]
fn launcher_arguments_use_dynamic_selected_models_without_containing_the_secret() {
    let args = build_direct_launcher_arguments(
        &settings(),
        Path::new(r"C:\Users\Test\AppData\Local\CodexAdministrator\instances\default"),
        true,
    )
    .unwrap();
    let rendered = args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(rendered.starts_with("inject --host direct"));
    assert!(rendered.contains("--launcher-managed"));
    assert!(rendered.contains("--model grok-4.5"));
    assert!(rendered.contains("--model grok-4.3-high"));
    assert!(rendered.contains("--base-url https://example.com/v1"));
    assert!(rendered.contains("--action-path /responses"));
    assert!(rendered.contains(&format!("--env-key {PROVIDER_RUNTIME_ENV_KEY}")));
    assert!(rendered.contains("--retain-instance-root"));
    assert!(rendered.contains("--sync-native-auth"));
    assert!(rendered.contains("--sync-native-sessions"));
    assert!(rendered.contains("--sync-native-goals"));
    assert!(rendered.contains("--sync-native-skills"));
    assert!(rendered.contains("--credential-present"));
    assert!(!rendered.contains("test-provider-secret"));
    assert!(!rendered.contains("CCSwitch"));
    assert!(!rendered.contains("HE BOX more"));
}

#[test]
fn launcher_arguments_follow_disabled_native_sync_settings() {
    let mut settings = settings();
    settings.sync_native_auth = false;
    settings.sync_native_sessions = false;
    settings.sync_native_goals = false;
    settings.sync_native_skills = false;
    let args = build_direct_launcher_arguments(&settings, Path::new(r"C:\isolated"), true).unwrap();
    let rendered = args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(!rendered.contains("--sync-native-auth"));
    assert!(!rendered.contains("--sync-native-sessions"));
    assert!(!rendered.contains("--sync-native-goals"));
    assert!(!rendered.contains("--sync-native-skills"));
}

#[test]
fn launcher_arguments_forward_an_editable_responses_action_path_without_touching_the_secret() {
    let mut settings = settings();
    settings.action_path = "/compatible/responses".into();
    settings.action_path_auto = false;

    let args = build_direct_launcher_arguments(&settings, Path::new(r"C:\isolated"), true).unwrap();
    let rendered = args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(rendered.contains("--base-url https://example.com/v1"));
    assert!(rendered.contains("--action-path /compatible/responses"));
    assert!(rendered.contains("--manual-action-path"));
}

#[test]
fn launcher_arguments_allow_management_only_startup_without_selected_models() {
    let generation = SupervisorGeneration::new(LauncherSettings::default(), None).unwrap();

    let args = build_direct_launcher_arguments(
        generation.launch_settings(),
        Path::new(r"C:\isolated"),
        false,
    )
    .unwrap();
    let rendered = args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(rendered.starts_with("inject --host direct"));
    assert!(!rendered.contains("--model"));
    assert!(rendered.contains("--base-url https://ai.hebox.net/v1"));
    assert!(rendered.contains("--action-path /responses"));
    assert!(rendered.contains("--sync-native-auth"));
    assert!(!rendered.contains("--sync-native-sessions"));
    assert!(!rendered.contains("--sync-native-goals"));
    assert!(rendered.contains("--sync-native-skills"));
    assert!(!rendered.contains("--credential-present"));
}

#[test]
fn management_only_arguments_report_a_saved_key_without_passing_a_model() {
    let args = build_direct_launcher_arguments(
        &LauncherSettings::default(),
        Path::new(r"C:\isolated"),
        true,
    )
    .unwrap();
    let rendered = args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(!rendered.contains("--model"));
    assert!(rendered.contains("--credential-present"));
}

#[test]
fn launcher_arguments_forward_only_enabled_reviewed_renderer_addons() {
    let mut settings = settings();
    settings.renderer_addons = vec![
        RendererAddonSettings {
            id: "codex-dream-skin".into(),
            enabled: true,
            source_root: r"C:\Injectors\Codex-Dream-Skin".into(),
        },
        RendererAddonSettings {
            id: "disabled-skin".into(),
            enabled: false,
            source_root: r"C:\Injectors\Disabled".into(),
        },
    ];

    let args = build_direct_launcher_arguments(&settings, Path::new(r"C:\isolated"), true).unwrap();
    let rendered = args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(rendered.contains("--renderer-addon codex-dream-skin=C:\\Injectors\\Codex-Dream-Skin"));
    assert!(!rendered.contains("disabled-skin"));
}

#[test]
fn management_only_child_spawn_accepts_an_absent_provider_credential() {
    let error = spawn_direct_launcher(
        Path::new(r"C:\definitely-missing\codex-administrator.exe"),
        &LauncherSettings::default(),
        Path::new(r"C:\isolated"),
        None,
        false,
    )
    .unwrap_err();

    assert!(error.to_string().contains("failed to launch"));
    assert!(!error.to_string().contains("provider API key is invalid"));
}

#[test]
fn launcher_scrubs_provider_and_common_secret_environment_variables() {
    for key in [
        "API_KEY",
        "CONNECTION_STRING",
        "OPENAI_API_KEY",
        "PASSWORD",
        "PAT",
        "SECRET",
        "TOKEN",
        "XAI_API_KEY",
        "HF_TOKEN",
        "GITHUB_TOKEN",
        "GITHUB_PAT",
        "AZURE_CLIENT_SECRET",
        "AZURE_STORAGE_CONNECTION_STRING",
        "DATABASE_PASSWORD",
        "DATABASE_URL",
        "MYSQL_PWD",
        "PGPASSWORD",
    ] {
        assert!(environment_variable_is_sensitive(key.as_ref()), "{key}");
    }
    for key in ["PATH", "USERPROFILE", "CODEX_HOME", "RUST_LOG"] {
        assert!(!environment_variable_is_sensitive(key.as_ref()), "{key}");
    }
}

#[test]
fn launcher_output_requires_the_structured_direct_ready_event() {
    assert!(launcher_output_is_ready(
        r#"{"status":"ready","host":"direct","mode":"configured","injection_enabled":true}"#
    ));
    assert!(!launcher_output_is_ready(
        r#"{"status":"ready","host":"direct","injection_enabled":true}"#
    ));
    assert!(!launcher_output_is_ready(
        r#"{"status":"validated","host":"direct","injection_enabled":false}"#
    ));
    assert!(!launcher_output_is_ready(
        r#"{"status":"ready","host":"codexplusplus","injection_enabled":true}"#
    ));
    assert!(!launcher_output_is_ready("not json"));
}

#[test]
fn launcher_diagnostics_are_bounded_and_redact_the_provider_key() {
    let secret = "test-provider-secret";
    let input = format!(
        "first line\nAuthorization: Bearer {secret}\nfailed with {secret}\0{}",
        "x".repeat(70_000)
    );
    let diagnostic = sanitize_launcher_diagnostic(input.as_bytes(), secret);

    assert!(!diagnostic.contains(secret));
    assert!(!diagnostic.contains("Authorization: Bearer"));
    assert!(!diagnostic.contains('\0'));
    assert!(diagnostic.len() <= 4096);
}
