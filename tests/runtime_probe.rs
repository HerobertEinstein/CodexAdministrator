use std::{path::PathBuf, time::Duration};

use codex_administrator::{RuntimeKind, RuntimeProbeStatus, probe_runtime_version};

#[tokio::test]
async fn probes_an_existing_runtime_without_a_shell() {
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_codex-administrator"));

    let result =
        probe_runtime_version(RuntimeKind::Codex, &executable, Duration::from_secs(5)).await;

    assert_eq!(result.kind, RuntimeKind::Codex);
    assert_eq!(result.status, RuntimeProbeStatus::Available);
    assert!(
        result
            .version
            .as_deref()
            .is_some_and(|value| value.contains(env!("CARGO_PKG_VERSION")))
    );
    assert!(result.error.is_none());
}

#[tokio::test]
async fn reports_a_missing_runtime_without_panicking() {
    let executable = PathBuf::from(r"C:\definitely-missing\codex.exe");

    let result =
        probe_runtime_version(RuntimeKind::Codex, &executable, Duration::from_secs(1)).await;

    assert_eq!(result.status, RuntimeProbeStatus::Missing);
    assert!(result.version.is_none());
    assert!(
        result
            .error
            .as_deref()
            .is_some_and(|value| value.contains("does not exist"))
    );
}
