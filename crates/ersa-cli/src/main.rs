//! The `ersa-core` executable: the Core process of ADR 0011. The Shell launches
//! it (or attaches to one already running), and remote hosts run the same binary.
//! It stays thin: argument parsing here, everything else in the `ersa-core` library.

use clap::{Parser, Subcommand};

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
    Serve,
}

fn main() {
    // `parse` exits the process itself on `--help`, `--version`, or bad
    // arguments, so past this line we always have a valid command.
    let cli = Cli::parse();

    match cli.command {
        Command::Serve => println!("not implemented"),
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
        assert!(matches!(cli.command, Command::Serve));
    }
}
