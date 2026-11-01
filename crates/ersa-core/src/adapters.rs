//! Adapter registry: which backends are installed on which hosts, their
//! versions and capabilities, and how to start them. The adapters themselves
//! live in their own crates, such as `ersa-adapter-claude`.
//!
//! So far this detects backends by asking each for its version through the
//! host's transport.

use std::io;
use std::time::Duration;

use ersa_adapter::{ProcessSpec, Transport};
use ersa_protocol::core_api::{BackendInfo, BackendState, BackendStatus};

use crate::transport::{self, RunError};

/// A backend Core knows how to drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBackend {
    /// The name on the wire, such as `claude-code`.
    pub name: &'static str,
    /// The executable the host resolves.
    pub program: &'static str,
    /// The range of versions Ersa was tested against.
    pub min_tested_version: &'static str,
    pub max_tested_version: &'static str,
}

/// Every backend Core knows. The tested range for Claude Code is the one in
/// `PROTO-CLAUDE-CODE-STREAM`; it moves to the adapter crate once that crate
/// exports it.
pub const KNOWN_BACKENDS: &[KnownBackend] = &[KnownBackend {
    name: "claude-code",
    program: "claude",
    min_tested_version: "2.1.302",
    max_tested_version: "2.1.302",
}];

/// How long a backend may take to print its version. Claude Code is a large
/// executable, and the first start after boot reads all of it from disk.
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);

/// What detection found for one backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub backend: KnownBackend,
    pub version: Option<String>,
    pub state: BackendState,
}

impl Detected {
    pub fn status(&self) -> BackendStatus {
        BackendStatus {
            name: self.backend.name.to_string(),
            version: self.version.clone(),
            status: self.state,
        }
    }

    pub fn info(&self) -> BackendInfo {
        BackendInfo {
            name: self.backend.name.to_string(),
            version: self.version.clone(),
            status: self.state,
            min_tested_version: self.backend.min_tested_version.to_string(),
            max_tested_version: self.backend.max_tested_version.to_string(),
        }
    }
}

/// Detects every known backend on the host behind `transport`, running
/// `<program> --version` for each. `program_override` replaces each backend's
/// program, which tests use to point at a binary that does not exist.
pub async fn detect(transport: &dyn Transport, program_override: Option<&str>) -> Vec<Detected> {
    let mut found = Vec::new();
    for backend in KNOWN_BACKENDS {
        let program = program_override.unwrap_or(backend.program);
        let (version, state) = match version_of(transport, program).await {
            Ok(Some(version)) => (Some(version), BackendState::Available),
            Ok(None) => (None, BackendState::Error),
            Err(RunError::Spawn(e)) if e.kind() == io::ErrorKind::NotFound => {
                (None, BackendState::Missing)
            }
            Err(e) => {
                tracing::warn!(backend = backend.name, error = %e, "backend detection failed");
                (None, BackendState::Error)
            }
        };
        found.push(Detected {
            backend: backend.clone(),
            version,
            state,
        });
    }
    found
}

/// The version `program --version` prints, or `None` when it ran but exited
/// with an error or printed nothing usable.
async fn version_of(transport: &dyn Transport, program: &str) -> Result<Option<String>, RunError> {
    let spec = ProcessSpec {
        program: program.to_string(),
        args: vec!["--version".to_string()],
        ..ProcessSpec::default()
    };
    let output = transport::run(transport, spec, VERSION_TIMEOUT).await?;
    if !output.exit.success() {
        return Ok(None);
    }
    Ok(output.stdout.first().and_then(|line| parse_version(line)))
}

/// The version number from a `--version` line: its first word when that
/// starts with a digit. Claude Code prints `2.1.302 (Claude Code)`.
fn parse_version(line: &str) -> Option<String> {
    let word = line.split_whitespace().next()?;
    word.starts_with(|c: char| c.is_ascii_digit())
        .then(|| word.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_from_the_first_word() {
        assert_eq!(
            parse_version("2.1.302 (Claude Code)").as_deref(),
            Some("2.1.302")
        );
        assert_eq!(parse_version("  1.0.0").as_deref(), Some("1.0.0"));
        assert_eq!(parse_version("Claude Code 2.1"), None);
        assert_eq!(parse_version(""), None);
    }

    #[tokio::test]
    async fn a_missing_program_is_reported_missing() {
        let found = detect(
            &ersa_adapter::LocalTransport,
            Some("ersa-no-such-program-xyz"),
        )
        .await;
        assert!(found.iter().all(|d| d.state == BackendState::Missing));
    }
}
