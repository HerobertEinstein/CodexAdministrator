use codex_administrator::{AgentMode, ModeState};

#[test]
fn serializes_the_two_public_main_agent_modes() {
    assert_eq!(
        serde_json::to_string(&AgentMode::GrokInjectedMain).unwrap(),
        r#""grok_injected_main""#
    );
    assert_eq!(
        serde_json::to_string(&AgentMode::NativeGptMain).unwrap(),
        r#""native_gpt_main""#
    );
}

#[test]
fn starts_in_native_gpt_mode_without_an_active_linked_task() {
    let state = ModeState::default();

    assert_eq!(state.mode, AgentMode::NativeGptMain);
    assert_eq!(state.revision, 0);
    assert!(state.task_id.is_none());
}

#[test]
fn mode_changes_increment_revision_and_preserve_the_task_link() {
    let mut state = ModeState::default();
    state.link_task("task-42").unwrap();
    state.set_mode(AgentMode::GrokInjectedMain);

    assert_eq!(state.mode, AgentMode::GrokInjectedMain);
    assert_eq!(state.revision, 2);
    assert_eq!(state.task_id.as_deref(), Some("task-42"));
}

#[test]
fn rejects_blank_or_oversized_task_ids() {
    let mut state = ModeState::default();

    assert!(state.link_task("  ").is_err());
    assert!(state.link_task(&"x".repeat(129)).is_err());
}
