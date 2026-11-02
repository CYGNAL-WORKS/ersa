//! Adapter registry: which backends are installed on which hosts, their
//! versions and capabilities, and how to start them. The adapters themselves
//! live in their own crates, such as `ersa-adapter-claude`.
//!
//! So far this detects backends by asking each for its version through the
//! host's transport.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{Adapter, BackendChoices, ProcessSpec, Transport};
use ersa_protocol::core_api::{BackendInfo, BackendState, BackendStatus, TestedRange};

use crate::transport::{self, RunError};

/// A backend Core knows how to drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBackend {
    /// The name on the wire, such as `claude-code`.
    pub name: &'static str,
    /// The executable the host resolves.
    pub program: &'static str,
}

/// Every backend Core knows. The range of versions each was tested with is
/// its adapter's ([`Adapter::tested_versions`]), so there is one source.
pub const KNOWN_BACKENDS: &[KnownBackend] = &[KnownBackend {
    name: "claude-code",
    program: "claude",
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
    /// The oldest and newest versions its adapter was tested with; empty
    /// when no adapter for it is registered.
    pub tested: (String, String),
    /// What a session of it can start with, from its adapter; none when no
    /// adapter for it is registered.
    pub choices: BackendChoices,
    /// Its display name, from its adapter; its name when no adapter for it
    /// is registered.
    pub label: String,
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
            label: self.label.clone(),
            version: self.version.clone(),
            status: self.state,
            tested: TestedRange {
                min: self.tested.0.clone(),
                max: self.tested.1.clone(),
            },
            models: self.choices.models.clone(),
            efforts: self.choices.efforts.clone(),
            permission_modes: self.choices.permission_modes.clone(),
        }
    }
}

/// Detects every known backend on the host behind `transport`, running
/// `<program> --version` for each. `program_override` replaces each backend's
/// program, which tests use to point at a binary that does not exist.
/// `adapter_for` names the adapter registered for a backend, whose
/// environment the check runs with, so it sees the CLI configured as the
/// backend's sessions see it.
pub async fn detect(
    transport: &dyn Transport,
    program_override: Option<&str>,
    adapter_for: impl Fn(&str) -> Option<Arc<dyn Adapter>>,
) -> Vec<Detected> {
    let mut found = Vec::new();
    for backend in KNOWN_BACKENDS {
        let mut spec = ProcessSpec {
            program: program_override.unwrap_or(backend.program).to_string(),
            args: vec!["--version".to_string()],
            ..ProcessSpec::default()
        };
        let adapter = adapter_for(backend.name);
        if let Some(adapter) = &adapter {
            adapter.set_environment(&mut spec);
        }
        let choices = adapter.as_ref().map(|a| a.choices()).unwrap_or_default();
        let label = adapter
            .as_ref()
            .map_or(backend.name, |a| a.label())
            .to_string();
        let tested = adapter
            .and_then(|a| a.tested_versions())
            .map(|(min, max)| (min.to_string(), max.to_string()))
            .unwrap_or_default();
        let (version, state) = match version_of(transport, spec).await {
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
            tested,
            choices,
            label,
        });
    }
    found
}

/// The version the `--version` command `spec` prints, or `None` when it ran
/// but exited with an error or printed nothing usable.
async fn version_of(
    transport: &dyn Transport,
    spec: ProcessSpec,
) -> Result<Option<String>, RunError> {
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
            |_| None,
        )
        .await;
        assert!(found.iter().all(|d| d.state == BackendState::Missing));
    }

    // verifies: PROTO-CLAUDE-CODE-STREAM-R19
    #[tokio::test]
    async fn the_version_check_runs_with_the_adapters_environment() {
        let transport = ersa_adapter::fake::FakeTransport::new(["2.1.316 (Claude Code)"]);
        let claude: Arc<dyn Adapter> = Arc::new(ersa_adapter_claude::ClaudeAdapter::new());
        let found = detect(&transport, None, |backend| {
            (backend == claude.backend()).then(|| claude.clone())
        })
        .await;
        assert_eq!(found[0].version.as_deref(), Some("2.1.316"));
        // The tested range and the choices are the adapter's own.
        let info = found[0].info();
        assert_eq!(
            (info.tested.min.as_str(), info.tested.max.as_str()),
            (
                ersa_adapter_claude::MIN_TESTED_VERSION,
                ersa_adapter_claude::MAX_TESTED_VERSION
            )
        );
        assert_eq!(info.models, claude.choices().models);
        assert_eq!(info.label, "Claude Code");

        let mut session = ProcessSpec::default();
        claude.set_environment(&mut session);
        let check = &transport.spawned()[0];
        assert_eq!(check.args, ["--version"]);
        assert_eq!(check.env, session.env);
        assert_eq!(check.env_remove, session.env_remove);
        assert!(
            check
                .env_remove
                .iter()
                .any(|n| n == "CLAUDE_CODE_SESSION_ID")
        );
    }
}
