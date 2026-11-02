//! The `ersa-core` executable: the Core process of ADR 0011. The Shell launches
//! it (or attaches to one already running), and remote hosts run the same binary.
//! It stays thin: argument parsing here, everything else in the `ersa-core` library.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand, ValueEnum};
use ersa_core::replay::{Pace, Recording, Replay};
use ersa_core::{CoreConfig, ServeError, Server};

/// Command-line arguments.
///
/// `#[derive(Parser)]` makes clap generate the parsing code from this struct.
/// `version` adds a `--version` flag that prints the crate version, which comes
/// from the workspace `Cargo.toml`. `name` overrides the crate name (`ersa-cli`)
/// so help and version output say `ersa-core`.
#[derive(Debug, Parser)]
#[command(name = "ersa-core", version, about = "The Ersa Core process")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// The subcommands. Each enum variant becomes one, named in kebab-case.
#[derive(Debug, Subcommand)]
enum Command {
    /// Run Core and serve the Interface protocols on a local socket.
    Serve {
        /// Where the database and worktrees live, instead of the user's
        /// application data directory.
        #[arg(long, value_name = "DIR")]
        data_dir: Option<PathBuf>,
        #[command(flatten)]
        endpoint: EndpointArg,
    },
    /// Serve one recorded session over the Interface protocols, with no
    /// backend: the replay server, for developing the Interface. It serves
    /// the label `replay` unless given another, never the user's own
    /// endpoint.
    Replay {
        /// The recording's stdout file (`<name>.jsonl`); its notes are the
        /// `<name>.meta.json` beside it.
        #[arg(long, value_name = "PATH")]
        fixture: PathBuf,
        /// `realtime` feeds each line when it arrived in the recording,
        /// counted from the first subscribe; `instant` feeds them all at once.
        #[arg(long, value_enum, default_value_t = PaceArg::Realtime)]
        pace: PaceArg,
        #[command(flatten)]
        endpoint: EndpointArg,
    },
}

/// The label a replay server serves when given none: never the user's own
/// endpoint, so an Interface reaches a replay only when told to, and a
/// replay never stands in for, or blocks, the user's Core.
const REPLAY_LABEL: &str = "replay";

/// `--pace`, as clap parses it. `ValueEnum` turns the variants into the
/// accepted values `realtime` and `instant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PaceArg {
    Realtime,
    Instant,
}

/// `--endpoint`, shared by every subcommand that serves the Core API
/// (`PROTO-CORE-API`, Transport and framing). `#[derive(Args)]` makes it a
/// group of arguments that a subcommand takes in with `#[command(flatten)]`.
#[derive(Debug, Default, PartialEq, Eq, Args)]
struct EndpointArg {
    /// Serve on the user's endpoint with this label appended (letters,
    /// digits, '-', '_'), instead of the user's own. For tests and a second
    /// Core, so neither collides with the one the Shell uses.
    #[arg(long, value_name = "LABEL")]
    endpoint: Option<String>,
}

impl EndpointArg {
    /// The labelled endpoint when a label was given, else `default`. Only a
    /// label is taken, never a raw pipe name or socket path, so an explicit
    /// endpoint is exactly as private as the default.
    fn or(self, default: String) -> std::io::Result<String> {
        match self.endpoint {
            Some(label) => ersa_core::platform::labelled_endpoint(&label),
            None => Ok(default),
        }
    }
}

// `#[tokio::main]` wraps `main` in a Tokio runtime, so it can be `async`.
#[tokio::main]
async fn main() -> ExitCode {
    // `parse` exits the process itself on `--help`, `--version`, or bad
    // arguments, so past this line we always have a valid command.
    let cli = Cli::parse();

    // Logs go to stderr. `RUST_LOG` (for example `RUST_LOG=ersa_core=debug`)
    // chooses what is shown; `info` and above by default.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        // Colour only on a terminal, so a log redirected to a file stays plain.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    match cli.command {
        Command::Serve { data_dir, endpoint } => serve(data_dir, endpoint).await,
        Command::Replay {
            fixture,
            pace,
            endpoint,
        } => replay(fixture, pace, endpoint).await,
    }
}

async fn serve(data_dir: Option<PathBuf>, endpoint: EndpointArg) -> ExitCode {
    let Some(data_dir) = data_dir.or_else(ersa_core::platform::default_data_dir) else {
        eprintln!("ersa-core: cannot find the application data directory; pass --data-dir");
        return ExitCode::FAILURE;
    };
    // Worktree paths are derived from the data directory and handed to git,
    // which runs in the project's directory; a relative path would land there.
    let data_dir = match std::path::absolute(&data_dir) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("ersa-core: invalid --data-dir {}: {e}", data_dir.display());
            return ExitCode::FAILURE;
        }
    };
    let mut config = match CoreConfig::new(data_dir) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("ersa-core: cannot name the endpoint to listen on: {e}");
            return ExitCode::FAILURE;
        }
    };
    config.endpoint = match endpoint.or(config.endpoint) {
        Ok(endpoint) => endpoint,
        Err(e) => {
            eprintln!("ersa-core: invalid --endpoint: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Every backend this build can drive. Core itself knows none of them.
    let adapters: Vec<Arc<dyn ersa_core::Adapter>> =
        vec![Arc::new(ersa_adapter_claude::ClaudeAdapter::new())];
    let server = match Server::bind_with(config, adapters).await {
        Ok(server) => server,
        Err(e @ ServeError::AlreadyRunning { .. }) => {
            eprintln!("ersa-core: {e}. Attach to it instead, or stop it first.");
            return ExitCode::FAILURE;
        }
        Err(e @ ServeError::NameTaken { .. }) => {
            eprintln!("ersa-core: {e}. Refusing to start, so clients never talk to that process.");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("ersa-core: {e}");
            return ExitCode::FAILURE;
        }
    };
    match server.run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ersa-core: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn replay(fixture: PathBuf, pace: PaceArg, endpoint: EndpointArg) -> ExitCode {
    let recording = match Recording::load(&fixture) {
        Ok(recording) => recording,
        Err(e) => {
            eprintln!("ersa-core: cannot replay {}: {e}", fixture.display());
            return ExitCode::FAILURE;
        }
    };
    // Only Claude Code recordings exist so far, and they note the CLI's
    // version under this key. Without it the replay would report an empty
    // version, a default in place of the recording, so it is refused.
    let backend_version = match recording.meta["claude_code_version"].as_str() {
        Some(version) if !version.is_empty() => version.to_string(),
        _ => {
            eprintln!(
                "ersa-core: cannot replay {}: the recording does not say which Claude Code version made it (`claude_code_version`)",
                fixture.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let endpoint = match endpoint.endpoint.as_deref() {
        Some(label) => ersa_core::platform::labelled_endpoint(label),
        None => ersa_core::platform::labelled_endpoint(REPLAY_LABEL),
    };
    let endpoint = match endpoint {
        Ok(endpoint) => endpoint,
        Err(e) => {
            eprintln!("ersa-core: invalid --endpoint: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Listen for Ctrl+C before anything needs cleaning up, so a Ctrl+C at
    // any point from here on removes the data directory below instead of
    // killing the process. The listener is registered when first polled,
    // which the biased select does now; `ready` lets it fall through.
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());
    tokio::select! {
        biased;
        signal = &mut ctrl_c => {
            // Done already: either Ctrl+C came, or it cannot be listened for.
            if let Err(e) = signal {
                eprintln!("ersa-core: cannot listen for Ctrl+C: {e}");
            }
            return ExitCode::FAILURE;
        }
        () = std::future::ready(()) => {}
    }
    // A fresh data directory per run: the replay records its session in a
    // database of its own, never the user's. `tempfile` makes it private to
    // the user (0700 on Unix) under an unguessable name, and removes it when
    // `data_dir` is dropped at the end of this function.
    let data_dir = match tempfile::Builder::new().prefix("ersa-replay-").tempdir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("ersa-core: cannot create a data directory for the replay: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut config = match CoreConfig::new(data_dir.path().to_path_buf()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("ersa-core: cannot name the endpoint to listen on: {e}");
            return ExitCode::FAILURE;
        }
    };
    config.endpoint = endpoint;
    let title = fixture
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "replay".into());
    let replay = Replay {
        adapter: Arc::new(ersa_adapter_claude::ClaudeAdapter::new()),
        recording,
        title,
        backend_version,
        pace: match pace {
            PaceArg::Realtime => Pace::Realtime,
            PaceArg::Instant => Pace::Instant,
        },
    };
    // Start-up races Ctrl+C: stopping before the server binds still reaches
    // the clean-up at the end of this function.
    let bound = tokio::select! {
        bound = Server::bind_replay(config, replay) => Some(bound),
        _ = &mut ctrl_c => None,
    };
    let code = match bound {
        None => ExitCode::FAILURE,
        Some(Ok(server)) => {
            tracing::info!("replaying {}", fixture.display());
            // Ctrl+C is how a replay is usually stopped: shut down as
            // core.shutdown does, so the data directory is removed below.
            // Held weakly: a strong hold would keep the database open, and
            // the directory with it, after the server has stopped.
            let core = Arc::downgrade(&server.core());
            tokio::spawn(async move {
                if ctrl_c.await.is_ok()
                    && let Some(core) = core.upgrade()
                {
                    core.request_shutdown();
                }
            });
            match server.run().await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("ersa-core: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some(Err(e @ ServeError::AlreadyRunning { .. })) => {
            eprintln!("ersa-core: {e}. Pass another --endpoint label to replay beside it.");
            ExitCode::FAILURE
        }
        Some(Err(e)) => {
            eprintln!("ersa-core: {e}");
            ExitCode::FAILURE
        }
    };
    // Best effort: on Windows a file still open (the database, if a task
    // outlived the server) keeps the directory, and it is left behind.
    if let Err(e) = data_dir.close() {
        tracing::warn!(error = %e, "could not remove the replay's data directory");
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// clap's own consistency check: catches mistakes in the attributes above,
    /// such as two arguments with the same name, at test time rather than at run time.
    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn serve_parses() {
        let cli = Cli::try_parse_from(["ersa-core", "serve"]).expect("serve is a valid command");
        assert!(matches!(
            cli.command,
            Command::Serve {
                data_dir: None,
                endpoint: EndpointArg { endpoint: None }
            }
        ));
    }

    #[test]
    fn serve_takes_an_endpoint() {
        let cli = Cli::try_parse_from(["ersa-core", "serve", "--endpoint", "second"])
            .expect("--endpoint is accepted");
        let Command::Serve { endpoint, .. } = cli.command else {
            panic!("serve parses as serve");
        };
        assert_eq!(
            endpoint.or("default".into()).unwrap(),
            ersa_core::platform::labelled_endpoint("second").unwrap()
        );
        assert_eq!(
            EndpointArg::default().or("default".into()).unwrap(),
            "default"
        );
    }

    #[test]
    fn serve_refuses_a_raw_pipe_name_or_path_as_endpoint() {
        for raw in [r"\\.\pipe\elsewhere", "/tmp/elsewhere.sock", "../up"] {
            let endpoint = EndpointArg {
                endpoint: Some(raw.into()),
            };
            assert!(endpoint.or("default".into()).is_err(), "{raw}");
        }
    }

    #[test]
    fn serve_takes_a_data_dir() {
        let cli = Cli::try_parse_from(["ersa-core", "serve", "--data-dir", "D:/tmp/ersa"])
            .expect("--data-dir is accepted");
        let Command::Serve { data_dir, .. } = cli.command else {
            panic!("serve parses as serve");
        };
        assert_eq!(data_dir, Some(PathBuf::from("D:/tmp/ersa")));
    }

    #[test]
    fn replay_takes_a_fixture_and_a_pace_that_defaults_to_realtime() {
        let cli = Cli::try_parse_from(["ersa-core", "replay", "--fixture", "f.jsonl"])
            .expect("replay parses");
        let Command::Replay {
            fixture,
            pace,
            endpoint,
        } = cli.command
        else {
            panic!("replay parses as replay");
        };
        assert_eq!(fixture, PathBuf::from("f.jsonl"));
        assert_eq!(pace, PaceArg::Realtime);
        assert_eq!(endpoint, EndpointArg::default());
        let cli = Cli::try_parse_from([
            "ersa-core",
            "replay",
            "--fixture",
            "f.jsonl",
            "--pace",
            "instant",
            "--endpoint",
            "mine",
        ])
        .expect("--pace instant and --endpoint are accepted");
        assert!(matches!(
            cli.command,
            Command::Replay {
                pace: PaceArg::Instant,
                ..
            }
        ));
    }

    #[test]
    fn the_replay_label_names_an_endpoint_that_is_not_the_users_own() {
        let replay = ersa_core::platform::labelled_endpoint(REPLAY_LABEL).unwrap();
        assert_ne!(replay, ersa_core::platform::default_endpoint().unwrap());
    }
}
