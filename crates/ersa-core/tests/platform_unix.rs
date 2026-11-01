//! Unix-only behaviour of the platform module: where the socket lives and
//! who may reach it (`PROTO-CORE-API`, Transport and framing).

// The whole file compiles only on Unix; elsewhere it is an empty test crate.
#![cfg(unix)]

mod support;

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use ersa_core::platform::{Listener, default_endpoint, labelled_endpoint, path_key};

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).expect("metadata").mode() & 0o777
}

#[test]
fn the_endpoint_is_in_the_runtime_dir_or_a_per_user_dir() {
    let endpoint = default_endpoint().expect("endpoint");
    let path = Path::new(&endpoint);
    assert_eq!(path.file_name().unwrap(), "ersa-core.sock");
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => assert_eq!(path.parent().unwrap(), Path::new(&dir)),
        None => {
            let dir = path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy();
            assert!(dir.starts_with("ersa-core-"), "{endpoint}");
        }
    }
}

#[test]
fn a_labelled_endpoint_is_beside_the_default_socket() {
    let default = default_endpoint().expect("endpoint");
    let endpoint = labelled_endpoint("second").expect("endpoint");
    let path = Path::new(&endpoint);
    assert_eq!(path.parent(), Path::new(&default).parent());
    assert_eq!(path.file_name().unwrap(), "ersa-core-second.sock");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_socket_is_private_and_removed_on_shutdown() {
    let core = support::start().await;
    let socket = Path::new(&core.endpoint).to_path_buf();
    assert_eq!(mode(&socket), 0o600);
    assert_eq!(
        mode(socket.parent().unwrap()) & 0o077,
        0,
        "directory admits nobody else"
    );

    let mut client = core.client().await;
    client.result("core.shutdown", serde_json::json!({})).await;
    core.task.await.expect("joined").expect("stopped cleanly");
    assert!(!socket.exists(), "the socket file is removed");
}

#[tokio::test]
async fn a_directory_others_can_enter_is_refused() {
    let dir = support::tempdir();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let endpoint = dir.path().join("core.sock").display().to_string();
    let error = Listener::bind(&endpoint).await.err().expect("refused");
    assert_ne!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("0700"), "{error}");
}

#[tokio::test]
async fn a_stale_socket_is_replaced() {
    let dir = support::tempdir();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let endpoint = dir.path().join("core.sock");
    // A socket file nobody accepts on, as a crashed Core leaves behind.
    drop(std::os::unix::net::UnixListener::bind(&endpoint).unwrap());
    assert!(endpoint.exists());

    let listener = Listener::bind(&endpoint.display().to_string()).await;
    assert!(listener.is_ok(), "{:?}", listener.err());
    assert_eq!(
        std::fs::metadata(&endpoint).unwrap().uid(),
        std::fs::metadata(dir.path()).unwrap().uid()
    );
}

#[tokio::test]
async fn a_file_that_is_not_a_socket_is_kept() {
    let dir = support::tempdir();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    // An explicit endpoint that names an ordinary file by mistake.
    let endpoint = dir.path().join("notes.txt");
    std::fs::write(&endpoint, "keep me").unwrap();

    let error = Listener::bind(&endpoint.display().to_string())
        .await
        .err()
        .expect("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists, "{error}");
    assert_eq!(std::fs::read_to_string(&endpoint).unwrap(), "keep me");
}

#[test]
fn path_keys_keep_case() {
    assert_eq!(path_key("/srv/repo/"), "/srv/repo");
    assert_ne!(path_key("/srv/Repo"), path_key("/srv/repo"));
    assert_eq!(path_key("/"), "/");
}
