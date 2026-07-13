use codex_administrator::{AgentMode, CompatibilityDecision, CompatibilityPolicy, HostAdapterKind};

fn policy() -> CompatibilityPolicy {
    CompatibilityPolicy::default()
        .allow(HostAdapterKind::Direct, "OpenAI.Codex/2026.713.1")
        .unwrap()
        .allow(HostAdapterKind::CodexPlusPlus, "1.2.35")
        .unwrap()
}

#[test]
fn verified_host_versions_may_enable_the_requested_main_agent_mode() {
    let decision = policy().evaluate(
        HostAdapterKind::CodexPlusPlus,
        Some("1.2.35"),
        AgentMode::GrokInjectedMain,
    );

    assert_eq!(
        decision,
        CompatibilityDecision::Enabled(AgentMode::GrokInjectedMain)
    );
}

#[test]
fn unknown_host_versions_fail_closed_to_native_gpt() {
    let decision = policy().evaluate(
        HostAdapterKind::CodexPlusPlus,
        Some("1.2.36"),
        AgentMode::GrokInjectedMain,
    );

    assert_eq!(
        decision,
        CompatibilityDecision::NativeOnly {
            requested: AgentMode::GrokInjectedMain,
            reason: "unverified_host_version".into(),
        }
    );
    assert_eq!(decision.effective_mode(), AgentMode::NativeGptMain);
}

#[test]
fn missing_version_evidence_never_enables_injection() {
    let decision = policy().evaluate(HostAdapterKind::Direct, None, AgentMode::GrokInjectedMain);

    assert_eq!(decision.effective_mode(), AgentMode::NativeGptMain);
    assert!(!decision.injection_enabled());
}

#[test]
fn native_gpt_remains_available_even_on_an_unverified_host() {
    let decision = policy().evaluate(
        HostAdapterKind::Direct,
        Some("future-build"),
        AgentMode::NativeGptMain,
    );

    assert_eq!(decision.effective_mode(), AgentMode::NativeGptMain);
    assert_eq!(
        decision,
        CompatibilityDecision::NativeOnly {
            requested: AgentMode::NativeGptMain,
            reason: "unverified_host_version".into(),
        }
    );
}

#[test]
fn policy_rejects_blank_version_entries() {
    assert!(
        CompatibilityPolicy::default()
            .allow(HostAdapterKind::Direct, "  ")
            .is_err()
    );
}
