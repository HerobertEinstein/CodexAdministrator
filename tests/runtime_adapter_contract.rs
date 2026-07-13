use std::{fs, path::PathBuf};

use codex_administrator::{
    RuntimeKind, RuntimeLaunchSpec, RuntimeProtocol, discover_codex_runtime_in,
};
use tempfile::tempdir;

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
    assert!(RuntimeLaunchSpec::validate_executable_path(&PathBuf::from("codex.exe")).is_err());
    assert!(
        RuntimeLaunchSpec::validate_executable_path(&PathBuf::from(r"C:\Tools\codex.cmd")).is_err()
    );
    assert!(
        RuntimeLaunchSpec::validate_executable_path(&PathBuf::from(r"C:\Tools\codex.exe")).is_ok()
    );
}

#[test]
fn discovers_official_npm_codex_as_node_plus_javascript_without_a_shell() {
    let temp = tempdir().unwrap();
    let node = temp.path().join("node.exe");
    let script = temp
        .path()
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("bin")
        .join("codex.js");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&node, b"fixture").unwrap();
    fs::write(&script, b"fixture").unwrap();

    let spec = discover_codex_runtime_in([temp.path().to_path_buf()]).unwrap();

    assert_eq!(spec.executable, node);
    assert_eq!(spec.args[0], script.to_string_lossy());
    assert_eq!(&spec.args[1..], ["app-server", "--stdio"]);
    assert!(!spec.use_shell);
}
