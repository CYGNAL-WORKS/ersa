//! Windows-only behaviour of the platform module: the pipe's name and
//! security descriptor, and path canonicalisation (`PROTO-CORE-API`,
//! Transport and framing).

// The whole file compiles only on Windows; elsewhere it is an empty test crate.
#![cfg(windows)]

mod support;

use std::path::Path;
use std::process::Command;

use ersa_core::platform::{canonicalize, default_endpoint, path_key};

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

#[tokio::test(flavor = "multi_thread")]
async fn the_pipe_admits_only_the_current_user() {
    let core = support::start().await;
    let name = core.endpoint.trim_start_matches(r"\\.\pipe\");
    // Read the pipe's access list through a client handle, from outside
    // Core: a protected list (`P`) with one entry allowing full access to
    // this user's SID.
    let script = format!(
        "$p = New-Object System.IO.Pipes.NamedPipeClientStream('.', '{name}', \
         [System.IO.Pipes.PipeDirection]::InOut); $p.Connect(5000); \
         $p.GetAccessControl().GetSecurityDescriptorSddlForm('Access'); $p.Dispose()"
    );
    let output = tokio::task::spawn_blocking(move || {
        Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .expect("powershell runs")
    })
    .await
    .expect("joined");
    let sddl = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(sddl, format!("D:P(A;;FA;;;{})", current_sid()));
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
