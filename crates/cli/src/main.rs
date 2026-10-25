// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme`, the DataBase.Me command-line tool.
//!
//! At milestone M0 it only reports version information. Vault commands (`import`, `export`,
//! `verify`, `query`, `mirror-check`) are added with the milestones that build them.

use clap::{Parser, Subcommand};

/// DataBase.Me command-line tool.
#[derive(Debug, Parser)]
#[command(name = "dbme", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the product, release and vault format versions.
    Info,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Info => print!("{}", info()),
    }
}

fn info() -> String {
    format!(
        "{} {}\nvault format: {} (draft)\n",
        dbme_core::PRODUCT_NAME,
        dbme_core::VERSION,
        dbme_core::FORMAT_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn info_reports_versions() {
        let text = info();
        assert!(text.starts_with("DataBase.Me "));
        assert!(text.contains("vault format: 0"));
    }
}
