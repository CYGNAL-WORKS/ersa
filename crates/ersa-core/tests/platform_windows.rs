//! Windows-only behaviour of the platform module: the pipe's name and
//! security descriptor, and path canonicalisation (`PROTO-CORE-API`,
//! Transport and framing); and environment names, which Windows compares
//! without case.

// The whole file compiles only on Windows; elsewhere it is an empty test crate.
#![cfg(windows)]

mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use ersa_adapter::ProcessSpec;
use ersa_core::transport::{LocalTreeTransport, run};

use ersa_core::platform::{canonicalize, default_endpoint, labelled_endpoint, path_key};

/// The current user's SID, as Windows itself reports it.
fn current_sid() -> String {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
        ])
        .output()
        .expect("powershell runs");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn the_endpoint_names_the_user_by_sid() {
    let endpoint = default_endpoint().expect("endpoint");
    assert_eq!(endpoint, format!(r"\\.\pipe\ersa-core-{}", current_sid()));
}

#[test]
fn a_labelled_endpoint_is_the_users_pipe_with_the_label() {
    let endpoint = labelled_endpoint("second").expect("endpoint");
    assert_eq!(
        endpoint,
        format!(r"\\.\pipe\ersa-core-{}-second", current_sid())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_pipe_admits_only_the_current_user() {
    let core = support::start().await;
    let name = core.endpoint.trim_start_matches(r"\\.\pipe\");
    // Read the pipe's access list through a client handle, from outside
    // Core: whether it is protected from inheritance, then each entry as
    // `<SID> <Allow|Deny> <rights>`. SIDs rather than SDDL text, because SDDL
    // abbreviates well-known accounts (the built-in Administrator is `LA`).
    let script = format!(
        "$p = New-Object System.IO.Pipes.NamedPipeClientStream('.', '{name}', \
         [System.IO.Pipes.PipeDirection]::InOut); $p.Connect(5000); \
         $a = $p.GetAccessControl(); $a.AreAccessRulesProtected; \
         $a.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]) | \
         ForEach-Object {{ \"$($_.IdentityReference.Value) $($_.AccessControlType) $([int]$_.PipeAccessRights)\" }}; \
         $p.Dispose()"
    );
    let output = tokio::task::spawn_blocking(move || {
        Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .expect("powershell runs")
    })
    .await
    .expect("joined");
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // 2032127 is 0x1F01FF, full control of a file or pipe.
    let expected = format!("{} Allow 2032127", current_sid());
    assert_eq!(lines, ["True", expected.as_str()], "{text}");
}

#[test]
fn canonical_paths_are_not_verbatim() {
    let dir = std::env::temp_dir();
    let canonical = canonicalize(&dir).expect("canonicalize");
    let text = canonical.display().to_string();
    assert!(!text.starts_with(r"\\?\"), "{text}");
    assert!(canonical.is_absolute());
    // Already canonical paths come back unchanged.
    assert_eq!(canonicalize(&canonical).expect("again"), canonical);
    // Letter case is the file system's, not the caller's.
    let shouted = Path::new(&text.to_uppercase()).to_path_buf();
    assert_eq!(canonicalize(&shouted).expect("upper case"), canonical);
}

#[test]
fn path_keys_ignore_case_and_separators() {
    assert_eq!(path_key(r"D:\Repo\"), path_key("d:/repo"));
    assert_ne!(path_key(r"D:\repo"), path_key(r"D:\repo2"));
}

/// Windows matches environment names without regard to case, so a removal
/// list written in capitals also removes a variable a parent spelled
/// otherwise, and the other way round: a lowercase name in the list removes
/// the capitalised `OS` every Windows process inherits. (From the review of
/// PR #36; the removal lists themselves are the adapter's to test.)
#[tokio::test]
async fn a_lowercase_name_in_the_removal_list_still_removes_the_variable() {
    assert!(std::env::var_os("OS").is_some(), "OS unset here");
    let spec = ProcessSpec {
        program: "cmd".into(),
        args: vec!["/C".into(), "echo [%OS%]".into()],
        env_remove: vec!["os".into()],
        ..ProcessSpec::default()
    };
    let output = run(&LocalTreeTransport::new(), spec, Duration::from_secs(30))
        .await
        .expect("cmd runs");
    // cmd leaves an unset variable's reference as it was written.
    assert_eq!(output.stdout[0].trim_end(), "[%OS%]");
}

/// A named pipe or a device reports itself as a file on Windows, so an
/// attachment or an inspect refuses it by its name before opening it.
// verifies: PROTO-CORE-API-R24
#[test]
fn pipes_and_devices_are_told_by_their_names() {
    use ersa_core::platform::names_device;
    for device in [
        r"\\.\pipe\some-pipe",
        r"\\?\pipe\some-pipe",
        r"C:\dir\NUL",
        r"C:\dir\com1.txt",
        r"C:\dir\LPT9",
    ] {
        assert!(names_device(Path::new(device)), "{device}");
    }
    for file in [
        r"C:\dir\notes.txt",
        r"C:\dir\console.txt",
        r"C:\dir\com0",
        r"\\?\C:\dir\notes.txt",
        r"\\server\share\notes.txt",
        r"\\?\UNC\server\share\notes.txt",
    ] {
        assert!(!names_device(Path::new(file)), "{file}");
    }
}
