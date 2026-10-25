// SPDX-License-Identifier: AGPL-3.0-or-later
//! DataBase.Me desktop entry point.

// Release builds on Windows are GUI apps: no console window behind the main window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let started = Instant::now();
    dbme_desktop::init_logging();
    match dbme_desktop::run(started) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "DataBase.Me failed to start");
            ExitCode::FAILURE
        }
    }
}
