use std::{collections::BTreeMap, env, path::PathBuf, time::Duration};

use codex_administrator::{RuntimeKind, RuntimeLaunchSpec, RuntimeProcess, RuntimeProtocol};
use serde_json::json;

#[tokio::test]
async fn launches_a_runtime_without_a_shell_and_separates_jsonl_from_stderr() {
    let executable = PathBuf::from(env::var("SystemRoot").unwrap())
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let script = concat!(
        "$line=[Console]::In.ReadLine();",
        "[Console]::Error.WriteLine('fixture-stderr');",
        "$message=$line|ConvertFrom-Json;",
        "$envValue=[Environment]::GetEnvironmentVariable('CODEX_ADMIN_TEST_ENV');",
        "$response=[ordered]@{id=$message.id;result=[ordered]@{ok=$true;env=$envValue}};",
        "[Console]::Out.WriteLine(($response|ConvertTo-Json -Compress -Depth 5));"
    );
    let spec = RuntimeLaunchSpec {
        kind: RuntimeKind::Codex,
        executable,
        args: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script.into(),
        ],
        env: BTreeMap::from([("CODEX_ADMIN_TEST_ENV".into(), "isolated".into())]),
        protocol: RuntimeProtocol::CodexAppServerJsonLines,
        use_shell: false,
    };

    let mut process = RuntimeProcess::spawn(spec, 64 * 1024).await.unwrap();
    let response = process
        .transport()
        .request(
            json!({"id":"request-1","method":"probe","params":{}}),
            Duration::from_secs(5),
        )
        .await
        .unwrap();

    assert_eq!(response["id"], "request-1");
    assert_eq!(response["result"]["ok"], true);
    assert_eq!(response["result"]["env"], "isolated");
    assert_eq!(
        process.stderr_mut().recv().await.as_deref(),
        Some("fixture-stderr")
    );
    assert!(process.wait().await.unwrap().success());
}

#[tokio::test]
async fn rejects_runtime_specs_that_request_shell_execution() {
    let spec = RuntimeLaunchSpec {
        kind: RuntimeKind::Codex,
        executable: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        args: vec![],
        env: BTreeMap::new(),
        protocol: RuntimeProtocol::CodexAppServerJsonLines,
        use_shell: true,
    };

    let error = match RuntimeProcess::spawn(spec, 4096).await {
        Ok(_) => panic!("shell execution should be rejected"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("shell execution"));
}
