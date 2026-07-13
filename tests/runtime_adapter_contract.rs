use std::path::PathBuf;

use codex_administrator::{RuntimeKind, RuntimeLaunchSpec, RuntimeProtocol};

#[test]
fn grok_uses_the_official_acp_stdio_agent_protocol() {
    let executable = PathBuf::from(r"C:\Tools\grok.exe");
    let spec = RuntimeLaunchSpec::grok(executable.clone());

    assert_eq!(spec.kind, RuntimeKind::Grok);
    assert_eq!(spec.executable, executable);
    assert_eq!(spec.args, ["agent", "--no-leader", "stdio"]);
    assert_eq!(spec.protocol, RuntimeProtocol::AcpV1JsonLines);
    assert!(!spec.use_shell);
}

#[test]
fn codex_uses_the_official_app_server_stdio_protocol() {
    let executable = PathBuf::from(r"C:\Tools\codex.exe");
    let spec = RuntimeLaunchSpec::codex(executable.clone());

    assert_eq!(spec.kind, RuntimeKind::Codex);
    assert_eq!(spec.executable, executable);
    assert_eq!(spec.args, ["app-server", "--stdio"]);
    assert_eq!(spec.protocol, RuntimeProtocol::CodexAppServerJsonLines);
    assert!(!spec.use_shell);
}

#[test]
fn runtime_specs_reject_non_executable_paths() {
    assert!(RuntimeLaunchSpec::validate_executable_path(&PathBuf::from("grok.exe")).is_err());
    assert!(
        RuntimeLaunchSpec::validate_executable_path(&PathBuf::from(r"C:\Tools\grok.cmd")).is_err()
    );
    assert!(
        RuntimeLaunchSpec::validate_executable_path(&PathBuf::from(r"C:\Tools\grok.exe")).is_ok()
    );
}
