use std::collections::VecDeque;

use anyhow::{Result, anyhow};
use codex_administrator::{
    DiscoveredModel, LauncherChildEvent, LauncherChildOutcome, LauncherSettings,
    LauncherSupervisorBackend, SupervisorExit, SupervisorGeneration, SupervisorMode,
    parse_launcher_child_event, supervise_launcher,
};

fn configured_settings() -> LauncherSettings {
    LauncherSettings {
        selected_models: vec!["grok-4.5".into()],
        cached_models: vec![DiscoveredModel {
            id: "grok-4.5".into(),
            owned_by: Some("xai".into()),
        }],
        ..LauncherSettings::default()
    }
}

#[test]
fn generation_enters_configured_mode_only_when_models_and_credential_exist() {
    let mut settings = configured_settings();
    settings.sync_native_sessions = true;
    let management = SupervisorGeneration::new(settings.clone(), None).unwrap();
    assert_eq!(management.mode(), SupervisorMode::ManagementOnly);
    assert!(management.settings().selected_models.is_empty());
    assert!(!management.settings().sync_native_sessions);
    assert!(management.credential().is_none());
    assert!(!management.credential_present());

    let management_with_saved_key = SupervisorGeneration::new(
        LauncherSettings::default(),
        Some("saved-provider-secret".into()),
    )
    .unwrap();
    assert_eq!(
        management_with_saved_key.mode(),
        SupervisorMode::ManagementOnly
    );
    assert!(management_with_saved_key.credential().is_none());
    assert!(management_with_saved_key.credential_present());

    let configured =
        SupervisorGeneration::new(settings, Some("provider-secret-that-must-not-leak".into()))
            .unwrap();
    assert_eq!(configured.mode(), SupervisorMode::Configured);
    assert_eq!(configured.settings().selected_models, ["grok-4.5"]);
    assert!(configured.settings().sync_native_sessions);
    assert_eq!(
        configured.credential(),
        Some("provider-secret-that-must-not-leak")
    );
    assert!(configured.credential_present());
    assert!(!format!("{configured:?}").contains("provider-secret-that-must-not-leak"));
}

#[test]
fn structured_child_events_distinguish_ready_restart_and_noise() {
    assert_eq!(
        parse_launcher_child_event(
            r#"{"status":"ready","host":"direct","mode":"management_only","injection_enabled":true}"#,
        ),
        Some(LauncherChildEvent::Ready {
            mode: SupervisorMode::ManagementOnly,
        })
    );
    assert_eq!(
        parse_launcher_child_event(
            r#"{"status":"restart_requested","host":"direct","instance_root":"C:\\isolated"}"#,
        ),
        Some(LauncherChildEvent::RestartRequested)
    );
    assert_eq!(parse_launcher_child_event("not json"), None);
    assert_eq!(
        parse_launcher_child_event(
            r#"{"status":"ready","host":"codexplusplus","mode":"configured","injection_enabled":true}"#,
        ),
        None
    );
}

struct FakeBackend {
    generations: VecDeque<SupervisorGeneration>,
    outcomes: VecDeque<LauncherChildOutcome>,
    observed_modes: Vec<SupervisorMode>,
}

impl LauncherSupervisorBackend for FakeBackend {
    fn load_generation(&mut self) -> Result<SupervisorGeneration> {
        self.generations
            .pop_front()
            .ok_or_else(|| anyhow!("missing generation"))
    }

    fn run_generation(
        &mut self,
        generation: &SupervisorGeneration,
    ) -> Result<LauncherChildOutcome> {
        self.observed_modes.push(generation.mode());
        self.outcomes
            .pop_front()
            .ok_or_else(|| anyhow!("missing outcome"))
    }
}

fn outcome(mode: SupervisorMode, restart_requested: bool, success: bool) -> LauncherChildOutcome {
    LauncherChildOutcome {
        ready_mode: Some(mode),
        restart_requested,
        success,
        exit_code: Some(if success { 0 } else { 1 }),
        diagnostic: String::new(),
    }
}

#[test]
fn supervisor_reloads_after_an_explicit_restart_then_stops_on_normal_close() {
    let mut backend = FakeBackend {
        generations: VecDeque::from([
            SupervisorGeneration::new(LauncherSettings::default(), None).unwrap(),
            SupervisorGeneration::new(configured_settings(), Some("provider-secret".into()))
                .unwrap(),
        ]),
        outcomes: VecDeque::from([
            outcome(SupervisorMode::ManagementOnly, true, true),
            outcome(SupervisorMode::Configured, false, true),
        ]),
        observed_modes: Vec::new(),
    };

    assert_eq!(
        supervise_launcher(&mut backend, 8).unwrap(),
        SupervisorExit::UserClosed
    );
    assert_eq!(
        backend.observed_modes,
        [SupervisorMode::ManagementOnly, SupervisorMode::Configured]
    );
}

#[test]
fn supervisor_fails_closed_on_abnormal_exit_or_restart_storm() {
    let mut failed = FakeBackend {
        generations: VecDeque::from([
            SupervisorGeneration::new(LauncherSettings::default(), None).unwrap()
        ]),
        outcomes: VecDeque::from([outcome(SupervisorMode::ManagementOnly, false, false)]),
        observed_modes: Vec::new(),
    };
    assert!(supervise_launcher(&mut failed, 8).is_err());

    let mut storm = FakeBackend {
        generations: (0..3)
            .map(|_| SupervisorGeneration::new(LauncherSettings::default(), None).unwrap())
            .collect(),
        outcomes: (0..3)
            .map(|_| outcome(SupervisorMode::ManagementOnly, true, true))
            .collect(),
        observed_modes: Vec::new(),
    };
    assert!(supervise_launcher(&mut storm, 2).is_err());
}
