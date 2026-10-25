// SPDX-License-Identifier: AGPL-3.0-or-later
//! The DataBase.Me desktop shell (Tauri 2).
//!
//! The shell opens one window with the web UI and gives it typed commands for the open vault, plus
//! a `vault-changed` event after every change (see [`ipc`], ADR-0014). When the UI has rendered it
//! calls `app_ready`, which logs the startup time.
//!
//! In smoke-test mode (CI's launch test, ADR-0008) the UI first creates a vault in a temporary
//! folder, creates an object and undoes it, all over IPC; `app_ready` then prints a marker and
//! exits.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tracing_subscriber::EnvFilter;

mod ipc;

/// Environment variable that turns on smoke-test mode. When it is set, the app exits as soon as
/// the UI reports it has rendered and run its launch checks: with status 0, after printing
/// [`SMOKE_OK_MARKER`] to stdout, if they passed, and with status 1 otherwise.
pub const SMOKE_TEST_ENV: &str = "DBME_SMOKE_TEST";

/// Line prefix printed to stdout in smoke-test mode once the UI is up and its checks passed,
/// followed by `startup_ms=N`.
pub const SMOKE_OK_MARKER: &str = "DBME_SMOKE_OK";

/// Line prefix printed to stdout in smoke-test mode when the UI's checks failed, followed by the
/// error.
pub const SMOKE_FAILED_MARKER: &str = "DBME_SMOKE_FAILED";

/// Startup bookkeeping shared with the commands.
pub(crate) struct Startup {
    started: Instant,
    /// In smoke-test mode, a temporary folder for the test vault and identity.
    smoke_dir: Option<PathBuf>,
    /// Removes that folder when dropped.
    smoke_temp: Mutex<Option<tempfile::TempDir>>,
    reported: AtomicBool,
}

/// Handles `app_ready`. Only the first call counts; React's StrictMode can call it twice in
/// development. `close` closes the vault before a smoke-test exit.
fn ready(app: &tauri::AppHandle, startup: &Startup, error: Option<String>, close: impl FnOnce()) {
    if startup.reported.swap(true, Ordering::SeqCst) {
        return;
    }
    let startup_ms = startup.started.elapsed().as_millis();
    match &error {
        None => tracing::info!(startup_ms, "UI ready"),
        Some(error) => {
            tracing::error!(startup_ms, %error, "UI ready, but its launch checks failed")
        }
    }
    if startup.smoke_dir.is_some() {
        close();
        // Remove the temporary folder now: exiting does not run destructors.
        drop(
            startup
                .smoke_temp
                .lock()
                .ok()
                .and_then(|mut dir| dir.take()),
        );
        match error {
            None => {
                println!("{SMOKE_OK_MARKER} startup_ms={startup_ms}");
                app.exit(0);
            }
            Some(error) => {
                println!("{SMOKE_FAILED_MARKER} {error}");
                app.exit(1);
            }
        }
    }
}

/// Sends logs to stderr. The level comes from `RUST_LOG` and defaults to `info`. Logs never contain
/// user content (handoff §2: no telemetry, diagnostics stay local).
pub fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    // A second initialisation (for example from a test harness) is harmless, so ignore it.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .try_init();
}

/// Builds and runs the Tauri app. `started` is when the process began, for the startup timing.
///
/// # Errors
/// Returns the Tauri error if the window or webview cannot be created, or an I/O error if
/// smoke-test mode cannot make its temporary folder.
pub fn run(started: Instant) -> Result<(), Box<dyn std::error::Error>> {
    let smoke_temp = match std::env::var_os(SMOKE_TEST_ENV) {
        Some(_) => Some(tempfile::Builder::new().prefix("dbme-smoke-").tempdir()?),
        None => None,
    };
    let startup = Startup {
        started,
        smoke_dir: smoke_temp.as_ref().map(|dir| dir.path().to_owned()),
        smoke_temp: Mutex::new(smoke_temp),
        reported: AtomicBool::new(false),
    };
    let builder = ipc::builder();
    tauri::Builder::default()
        .manage(startup)
        .manage(ipc::Session::default())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// tauri.conf.json takes the app version from apps/desktop/package.json, while the Rust crates
    /// take it from the workspace Cargo.toml. They must never disagree.
    #[test]
    fn package_json_version_matches_cargo_version() {
        let package_json = include_str!("../../package.json");
        let parsed: serde_json::Value = serde_json::from_str(package_json).unwrap();
        assert_eq!(parsed["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(env!("CARGO_PKG_VERSION"), dbme_core::VERSION);
    }
}
