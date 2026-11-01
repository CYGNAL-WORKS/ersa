//! Core's command runner over the real local transport.

use std::time::Duration;

use ersa_adapter::{LocalTransport, ProcessSpec};
use ersa_core::transport::{RunError, run};

/// A command whose output has a line that is not UTF-8 between two that
/// are. On Windows, PowerShell writes the raw bytes; elsewhere, printf.
fn mixed_output_command() -> ProcessSpec {
    let (program, args): (&str, Vec<&str>) = if cfg!(windows) {
        (
            "powershell",
            vec![
                "-NoProfile",
                "-Command",
                "$o = [Console]::OpenStandardOutput(); \
                 $b = [byte[]](0x61,0x0a,0xff,0xfe,0x0a,0x62,0x0a); \
                 $o.Write($b, 0, $b.Length); $o.Flush()",
            ],
        )
    } else {
        ("printf", vec!["a\\n\\377\\376\\nb\\n"])
    };
    ProcessSpec {
        program: program.into(),
        args: args.into_iter().map(str::to_string).collect(),
        ..ProcessSpec::default()
    }
}

#[tokio::test]
async fn a_line_that_is_not_utf8_does_not_fail_the_command() {
    let output = run(
        &LocalTransport,
        mixed_output_command(),
        Duration::from_secs(60),
    )
    .await
    .expect("the command completes");
    assert!(output.exit.success());
    // The middle line is the bytes 0xFF 0xFE, decoded the way the
    // transport's line reader decodes anything that is not UTF-8.
    let middle = String::from_utf8_lossy(&[0xff, 0xfe]).into_owned();
    assert!(middle.contains('\u{FFFD}'));
    assert_eq!(output.stdout, ["a", middle.as_str(), "b"]);
}

#[tokio::test]
async fn a_missing_program_is_a_spawn_error() {
    let spec = ProcessSpec {
        program: "ersa-test-no-such-program".into(),
        ..ProcessSpec::default()
    };
    let result = run(&LocalTransport, spec, Duration::from_secs(5)).await;
    assert!(matches!(result, Err(RunError::Spawn(e)) if e.kind() == std::io::ErrorKind::NotFound));
}
