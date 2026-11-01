//! The `ersa-core` executable: the Core process of ADR 0011. The Shell launches
//! it (or attaches to one already running), and remote hosts run the same binary.
//! It stays thin: argument parsing here, everything else in the `ersa-core` library.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Args, Parser, Subcommand};
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
}

/// `--endpoint`, shared by every subcommand that serves the Core API
/// (`PROTO-CORE-API`, Transport and framing). `#[derive(Args)]` makes it a
/// group of arguments that a subcommand takes in with `#[command(flatten)]`.
#[derive(Debug, Default, PartialEq, Eq, Args)]
struct EndpointArg {
    /// The named pipe or socket to serve on, instead of the user's default.
    /// For tests and a second Core, so neither collides with the one the
    /// Shell uses.
    #[arg(long, value_name = "NAME")]
    endpoint: Option<String>,
}

impl EndpointArg {
    /// The endpoint given, or `default` (the user's own) when none was.
    fn or(self, default: String) -> String {
        self.endpoint.unwrap_or(default)
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
    config.endpoint = endpoint.or(config.endpoint);
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
        let cli = Cli::try_parse_from(["ersa-core", "serve", "--endpoint", "elsewhere"])
            .expect("--endpoint is accepted");
        let Command::Serve { endpoint, .. } = cli.command;
        assert_eq!(endpoint.or("default".into()), "elsewhere");
        assert_eq!(EndpointArg::default().or("default".into()), "default");
    }

    #[test]
    fn serve_takes_a_data_dir() {
        let cli = Cli::try_parse_from(["ersa-core", "serve", "--data-dir", "D:/tmp/ersa"])
            .expect("--data-dir is accepted");
        let Command::Serve { data_dir, .. } = cli.command;
        assert_eq!(data_dir, Some(PathBuf::from("D:/tmp/ersa")));
    }
}
