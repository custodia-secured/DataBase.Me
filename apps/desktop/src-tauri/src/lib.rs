// SPDX-License-Identifier: AGPL-3.0-or-later
//! The DataBase.Me desktop shell (Tauri 2).
//!
//! At milestone M0 the shell opens one window with the web UI and exposes a single command,
//! `app_ready`, which the UI calls once it has rendered. That gives a startup-time measurement and
//! lets CI prove the app launches on every OS (ADR-0008).

use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tracing_subscriber::EnvFilter;

/// Environment variable that turns on smoke-test mode. When it is set, the app exits with status 0
/// as soon as the UI reports it has rendered, after printing [`SMOKE_OK_MARKER`] to stdout.
pub const SMOKE_TEST_ENV: &str = "DBME_SMOKE_TEST";

/// Line prefix printed to stdout in smoke-test mode once the UI is up, followed by `startup_ms=N`.
pub const SMOKE_OK_MARKER: &str = "DBME_SMOKE_OK";

/// Startup bookkeeping shared with the `app_ready` command.
struct Startup {
    started: Instant,
    smoke_test: bool,
    reported: AtomicBool,
}

/// Called by the UI after its first render. Logs how long startup took and, in smoke-test mode,
/// exits the app. Only the first call counts; React's StrictMode can call it twice in development.
#[tauri::command]
fn app_ready(app: tauri::AppHandle, startup: tauri::State<'_, Startup>) {
    if startup.reported.swap(true, Ordering::SeqCst) {
        return;
    }
    let startup_ms = startup.started.elapsed().as_millis();
    tracing::info!(startup_ms, "UI ready");
    if startup.smoke_test {
        println!("{SMOKE_OK_MARKER} startup_ms={startup_ms}");
        app.exit(0);
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
/// Returns the Tauri error if the window or webview cannot be created.
pub fn run(started: Instant) -> Result<(), tauri::Error> {
    let startup = Startup {
        started,
        smoke_test: std::env::var_os(SMOKE_TEST_ENV).is_some(),
        reported: AtomicBool::new(false),
    };
    tauri::Builder::default()
        .manage(startup)
        .invoke_handler(tauri::generate_handler![app_ready])
        .run(tauri::generate_context!())
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
