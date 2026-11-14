//! Adapter registry: which backends are installed on which hosts, their
//! versions and capabilities, and how to start them. The adapters themselves
//! live in their own crates, one per backend, and the `ersa-core` binary
//! registers them: the registered adapters are the only
//! backends Core knows, and it names none of them (ADR 0031).
//!
//! Detection runs each adapter's version command through the host's
//! transport, then asks a backend it found for its models.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use ersa_adapter::{Adapter, AdapterError, BackendChoices, Transport};
use ersa_protocol::core_api::{BackendInfo, BackendState, BackendStatus, ModelChoice, TestedRange};

use crate::transport::{self, RunError};

/// How long a backend may take to print its version. Claude Code is a large
/// executable, and the first start after boot reads all of it from disk.
const VERSION_TIMEOUT: Duration = Duration::from_secs(20);

/// What detection found for one backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// The backend's name on the wire, such as `claude-code`.
    pub name: String,
    pub version: Option<String>,
    pub state: BackendState,
    /// The oldest and newest versions its adapter was tested with; empty
    /// when the adapter does not say.
    pub tested: (String, String),
    /// The efforts and permission modes a session of it can start with,
    /// from its adapter.
    pub choices: BackendChoices,
    /// Its models, as the backend reported them on the host; none unless
    /// it is available (PROTO-CORE-API-R72).
    pub models: Vec<ModelChoice>,
    /// Its display name, from its adapter.
    pub label: String,
}

impl Detected {
    pub fn status(&self) -> BackendStatus {
        BackendStatus {
            name: self.name.clone(),
            version: self.version.clone(),
            status: self.state,
        }
    }

    pub fn info(&self) -> BackendInfo {
        BackendInfo {
            name: self.name.clone(),
            label: self.label.clone(),
            version: self.version.clone(),
            status: self.state,
            tested: TestedRange {
                min: self.tested.0.clone(),
                max: self.tested.1.clone(),
            },
            models: self.models.clone(),
            efforts: self.choices.efforts.clone(),
            permission_modes: self.choices.permission_modes.clone(),
        }
    }
}

/// Detects every registered adapter's backend on the host behind
/// `transport`, in the order the adapters were registered
/// (PROTO-CORE-API-R65), running each one's version command with its
/// environment, so the check sees the CLI configured as the backend's
/// sessions see it. `program_override` replaces each command's program,
/// which tests use to point at a binary that does not exist.
///
/// A backend found asks for its models through `probe`, the transport its
/// sessions start through, since the backend may start processes of its
/// own that must end with it. One that cannot report them is `error`, and
/// one the probe cannot find `missing`, either with no models
/// (PROTO-CORE-API-R72).
pub async fn detect(
    transport: &dyn Transport,
    probe: Arc<dyn Transport>,
    program_override: Option<&str>,
    adapters: &[Arc<dyn Adapter>],
) -> Vec<Detected> {
    let mut found = Vec::new();
    for adapter in adapters {
        let mut spec = adapter.version_command();
        if let Some(program) = program_override {
            spec.program = program.to_string();
        }
        adapter.set_environment(&mut spec);
        let (version, state) = match transport::run(transport, spec, VERSION_TIMEOUT).await {
            // A command that exited with an error, or printed nothing the
            // adapter can read, found a backend that does not work.
            Ok(output) => match output
                .exit
                .success()
                .then(|| adapter.read_version(&output.stdout))
                .flatten()
            {
                Some(version) => (Some(version), BackendState::Available),
                None => (None, BackendState::Error),
            },
            Err(RunError::Spawn(e)) if e.kind() == io::ErrorKind::NotFound => {
                (None, BackendState::Missing)
            }
            Err(e) => {
                tracing::warn!(backend = adapter.backend(), error = %e, "backend detection failed");
                (None, BackendState::Error)
            }
        };
        let (state, models) = if state == BackendState::Available {
            match adapter.models(Arc::clone(&probe)).await {
                Ok(models) => (state, models),
                Err(AdapterError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                    (BackendState::Missing, Vec::new())
                }
                Err(e) => {
                    tracing::warn!(backend = adapter.backend(), error = %e, "backend could not list its models");
                    (BackendState::Error, Vec::new())
                }
            }
        } else {
            (state, Vec::new())
        };
        found.push(Detected {
            name: adapter.backend().to_string(),
            version,
            state,
            models,
            tested: adapter
                .tested_versions()
                .map(|(min, max)| (min.to_string(), max.to_string()))
                .unwrap_or_default(),
            choices: adapter.choices(),
            label: adapter.label().to_string(),
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend with nothing but a name and the default version command.
    struct Probe;

    #[ersa_adapter::async_trait]
    impl Adapter for Probe {
        fn backend(&self) -> &str {
            "probe"
        }
        fn title_rules(&self) -> ersa_adapter::TitleRules {
            ersa_adapter::TitleRules {
                max_chars: 10,
                one_line: true,
            }
        }
        async fn start(
            &self,
            _: Arc<dyn Transport>,
            _: ersa_adapter::SessionConfig,
            _: Arc<dyn ersa_adapter::AdapterSink>,
        ) -> Result<Box<dyn ersa_adapter::AdapterSession>, ersa_adapter::AdapterError> {
            unreachable!("detection never starts a session")
        }
        fn projector(&self) -> Box<dyn ersa_adapter::Projector> {
            unreachable!("detection projects nothing")
        }
    }

    #[tokio::test]
    async fn a_missing_program_is_reported_missing() {
        let probe: Arc<dyn Adapter> = Arc::new(Probe);
        let found = detect(
            &ersa_adapter::LocalTransport,
            Arc::new(ersa_adapter::LocalTransport),
            Some("ersa-no-such-program-xyz"),
            &[probe],
        )
        .await;
        assert_eq!(found.len(), 1);
        assert!(found.iter().all(|d| d.state == BackendState::Missing));
    }
}
