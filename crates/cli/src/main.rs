// SPDX-License-Identifier: AGPL-3.0-or-later
//! `dbme`, the DataBase.Me command-line tool.
//!
//! M1 adds `vault create` and `verify`. The other vault commands (`import`, `export`, `query`,
//! `mirror-check`) arrive with the milestones that build them.
//!
//! Exit status: 0 on success, 1 if `verify` found problems, 2 if the command could not run.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use dbme_core::SystemClock;
use dbme_store::{Identity, Vault, VaultOptions};

/// The desktop app's identifier. The CLI keeps its identity in the same folder as the app, so
/// both record the same author in the op log.
const APP_IDENTIFIER: &str = "org.databaseme.desktop";

/// Overrides the folder that holds `identity.json`.
const CONFIG_DIR_ENV: &str = "DBME_CONFIG_DIR";

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
    /// Create and manage vaults.
    #[command(subcommand)]
    Vault(VaultCommand),
    /// Check a vault: SQLite's own checks, the op log's hash chain, a full replay of the log
    /// compared with every table, and the blob store.
    Verify {
        /// The vault folder (`<name>.dbme`).
        path: PathBuf,
        /// Also read every blob back and check its content against its hash.
        #[arg(long)]
        deep: bool,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum VaultCommand {
    /// Create a new vault: a folder named `<name>.dbme` holding the built-in types.
    Create {
        /// Where to create it. The folder must not exist yet, or be empty.
        path: PathBuf,
    },
}

type Failure = Box<dyn std::error::Error>;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = config_dir().and_then(|config| run(cli.command, &config, &mut std::io::stdout()));
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("dbme: {error}");
            ExitCode::from(2)
        }
    }
}

/// Where the app keeps its configuration: `DBME_CONFIG_DIR`, or the platform's configuration
/// folder followed by the app identifier, which is where the desktop app looks too.
fn config_dir() -> Result<PathBuf, Failure> {
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
        return Ok(dir.into());
    }
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library").join("Application Support"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")))
    };
    base.map(|base| base.join(APP_IDENTIFIER))
        .ok_or_else(|| format!("cannot find a configuration folder; set {CONFIG_DIR_ENV}").into())
}

fn options(config: &Path) -> Result<VaultOptions, Failure> {
    let clock = Arc::new(SystemClock);
    let identity = Identity::load_or_create(config, clock.as_ref())?;
    Ok(VaultOptions { identity, clock })
}

fn run(command: Command, config: &Path, out: &mut impl Write) -> Result<ExitCode, Failure> {
    match command {
        Command::Info => write!(out, "{}", info())?,
        Command::Vault(VaultCommand::Create { path }) => {
            let vault = Vault::create(&path, options(config)?)?;
            writeln!(out, "Created vault {} at {}", vault.id(), path.display())?;
        }
        Command::Verify { path, deep, json } => {
            let vault = Vault::open(&path, options(config)?)?;
            let report = vault.verify(deep)?;
            if json {
                writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
            } else {
                writeln!(out, "Vault {} ({})", vault.id(), path.display())?;
                writeln!(
                    out,
                    "schema version {}, {} ops, {} objects, {} blobs",
                    report.schema_version, report.ops, report.objects, report.blobs
                )?;
                if let Some(head) = &report.head {
                    writeln!(out, "chain head {head}")?;
                }
                if report.is_ok() {
                    let blobs = if deep {
                        "present and intact"
                    } else {
                        "present with the right sizes"
                    };
                    writeln!(
                        out,
                        "OK: the hash chain holds, replaying the op log reproduces every table, and every blob is {blobs}."
                    )?;
                } else {
                    writeln!(out, "{} problems:", report.problems.len())?;
                    for problem in &report.problems {
                        writeln!(out, "- {problem}")?;
                    }
                }
            }
            if !report.is_ok() {
                return Ok(ExitCode::FAILURE);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
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

    fn run_text(command: Command, config: &Path) -> (Result<ExitCode, String>, String) {
        let mut out = Vec::new();
        let code = run(command, config, &mut out).map_err(|e| e.to_string());
        (code, String::from_utf8(out).unwrap())
    }

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(args).unwrap().command
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn info_reports_versions() {
        let text = info();
        assert!(text.starts_with("DataBase.Me "));
        assert!(text.contains("vault format: 0"));
        let dir = tempfile::tempdir().unwrap();
        let (code, out) = run_text(parse(&["dbme", "info"]), dir.path());
        assert_eq!(code, Ok(ExitCode::SUCCESS));
        assert_eq!(out, text);
    }

    #[test]
    fn create_then_verify() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let vault = dir.path().join("Notes.dbme");
        let vault_arg = vault.to_str().unwrap();

        let (code, out) = run_text(parse(&["dbme", "vault", "create", vault_arg]), &config);
        assert_eq!(code, Ok(ExitCode::SUCCESS));
        assert!(out.starts_with("Created vault "), "{out}");
        assert!(config.join(dbme_store::IDENTITY_FILE).is_file());

        let (code, out) = run_text(parse(&["dbme", "verify", "--deep", vault_arg]), &config);
        assert_eq!(code, Ok(ExitCode::SUCCESS));
        assert!(
            out.contains("25 ops") && out.contains("OK:") && out.contains("intact"),
            "{out}"
        );

        let (code, out) = run_text(parse(&["dbme", "verify", "--json", vault_arg]), &config);
        assert_eq!(code, Ok(ExitCode::SUCCESS));
        let report: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(report["ops"], 25);
        assert_eq!(report["problems"], serde_json::json!([]));

        // A damaged vault fails verification with status 1 and lists the problems.
        let expected = damage(&vault);
        let (code, out) = run_text(parse(&["dbme", "verify", vault_arg]), &config);
        assert_eq!(code, Ok(ExitCode::FAILURE), "{out}");
        assert!(
            out.contains("problems:") && out.contains(&expected),
            "{out}"
        );

        // Errors are reported, not panicked on.
        let (code, _) = run_text(parse(&["dbme", "vault", "create", vault_arg]), &config);
        assert!(code.unwrap_err().contains("already exists"));
        let missing = dir.path().join("Missing.dbme");
        let (code, _) = run_text(
            parse(&["dbme", "verify", missing.to_str().unwrap()]),
            &config,
        );
        assert!(code.unwrap_err().contains("not a DataBase.Me vault"));
    }

    /// Damages a vault without SQL: removes the copy of a blob the database records. Returns the
    /// text the report must contain.
    fn damage(vault: &Path) -> String {
        let clock = Arc::new(dbme_core::ManualClock::new(1));
        let identity = Identity::generate(clock.as_ref()).unwrap();
        let mut open = Vault::open(vault, VaultOptions { identity, clock }).unwrap();
        let blob = open.blobs().put(b"scan").unwrap();
        open.transact(|txn| {
            let id = txn.create_object(dbme_store::NewObject {
                title: "with a scan".into(),
                ..Default::default()
            })?;
            txn.attach(dbme_store::NewAttachment {
                object_id: id,
                blob_hash: blob.hash.clone(),
                role: dbme_core::model::AttachmentRole::Original,
                filename: None,
                mime: None,
            })
        })
        .unwrap();
        std::fs::remove_file(open.blobs().path(&blob.hash)).unwrap();
        format!("blob {} is missing", blob.hash)
    }

    #[test]
    fn the_config_folder_can_be_overridden() {
        // Only the default lookup is exercised here: setting environment variables in tests is
        // not thread-safe.
        let dir = config_dir().unwrap();
        assert!(
            std::env::var_os(CONFIG_DIR_ENV).is_some() || dir.ends_with(APP_IDENTIFIER),
            "{}",
            dir.display()
        );
    }
}
