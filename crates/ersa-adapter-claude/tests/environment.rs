//! The session's environment against a real child process: variables that tie
//! a process to another host's session are absent from the CLI's environment
//! even when the process that started Ersa has them.
//!
//! A test cannot safely set variables in its own process (other tests run on
//! other threads), so the outer test runs this test binary again with the
//! variables set, and the inner test, ignored otherwise, does the check there.

use std::time::Duration;

use ersa_adapter::{Adapter, LocalTransport, ProcessSpec, Transport};
use ersa_adapter_claude::ClaudeAdapter;

/// PROTO-CLAUDE-CODE-STREAM-R19's names.
const HOST_SESSION_VARIABLES: [&str; 7] = [
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SSE_PORT",
    "ENABLE_IDE_INTEGRATION",
    "CLAUDE_AGENT_SDK_VERSION",
];

/// The value the outer test gives each variable, found nowhere else.
const PARENT_VALUE: &str = "ersa-parent-value-7f3c";

const INNER: &str = "inner_the_child_sees_none_of_the_parents_session_variables";

// verifies: PROTO-CLAUDE-CODE-STREAM-R19
#[test]
fn another_hosts_session_variables_never_reach_the_cli() {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        INNER,
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    for name in HOST_SESSION_VARIABLES {
        command.env(name, PARENT_VALUE);
    }
    let output = command.output().expect("the test binary runs again");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The inner test ran, rather than being filtered away.
    assert!(stdout.contains("checked 7 variables"), "{stdout}");
}

#[tokio::test]
#[ignore = "run by another_hosts_session_variables_never_reach_the_cli, with the variables set"]
async fn inner_the_child_sees_none_of_the_parents_session_variables() {
    // Run on its own (by `--ignored`, say), there is nothing to check.
    if std::env::var_os(HOST_SESSION_VARIABLES[0]).is_none() {
        return;
    }
    for name in HOST_SESSION_VARIABLES {
        assert_eq!(
            std::env::var(name).as_deref(),
            Ok(PARENT_VALUE),
            "{name} not set on the parent"
        );
    }
    // The child prints its whole environment.
    let mut spec = if cfg!(windows) {
        ProcessSpec {
            program: "cmd".into(),
            args: vec!["/C".into(), "set".into()],
            ..ProcessSpec::default()
        }
    } else {
        ProcessSpec {
            program: "env".into(),
            ..ProcessSpec::default()
        }
    };
    ClaudeAdapter::new().set_environment(&mut spec);

    let mut process = LocalTransport.spawn(spec).await.expect("spawn");
    drop(process.take_stdin());
    let mut stdout = process.take_stdout().expect("stdout");
    let mut seen = Vec::new();
    let read = async {
        while let Some(line) = stdout.next_line().await.expect("read") {
            seen.push(line);
        }
    };
    tokio::time::timeout(Duration::from_secs(30), read)
        .await
        .expect("the child finishes printing");
    process.wait().await.expect("exits");

    let leaked: Vec<&String> = seen.iter().filter(|l| l.contains(PARENT_VALUE)).collect();
    assert!(leaked.is_empty(), "reached the child: {leaked:?}");
    assert!(
        seen.iter()
            .any(|l| l.trim_end() == "CLAUDE_CODE_ENTRYPOINT=ersa"),
        "the child's environment was read"
    );
    println!("checked {} variables", HOST_SESSION_VARIABLES.len());
}
