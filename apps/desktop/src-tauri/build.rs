// SPDX-License-Identifier: AGPL-3.0-or-later
//! Build script: generates the Tauri context and the permissions for the app's own commands.
//!
//! Listing the commands in an app manifest makes them deny-by-default: a window can call a command
//! only if one of its capabilities grants the generated `allow-<command>` permission (ADR-0010).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = tauri_build::AppManifest::new().commands(&["app_ready"]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))?;
    Ok(())
}
