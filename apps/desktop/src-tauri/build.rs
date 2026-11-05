// SPDX-License-Identifier: AGPL-3.0-or-later
//! Build script: generates the Tauri context and the permissions for the app's own commands.
//!
//! Listing the commands in an app manifest makes them deny-by-default: a window can call a command
//! only if one of its capabilities grants the generated `allow-<command>` permission (ADR-0010).

/// Every command in `src/ipc.rs`. A test there checks this list against the commands it registers.
const COMMANDS: &[&str] = &[
    "app_info",
    "app_ready",
    "vault_create",
    "vault_open",
    "vault_open_default",
    "vault_close",
    "vault_info",
    "undo",
    "redo",
    "types_list",
    "fields_list",
    "field_create",
    "column_target",
    "column_add",
    "field_rename",
    "field_delete",
    "row_add",
    "object_get",
    "object_children",
    "object_ancestors",
    "object_summaries",
    "objects_page",
    "objects_count",
    "object_search",
    "backlinks",
    "object_tree",
    "search",
    "objects_query",
    "objects_query_count",
    "facets",
    "aggregate",
    "data_profile",
    "graph_read",
    "views_list",
    "view_get",
    "view_save",
    "view_update",
    "view_delete",
    "relation_create",
    "relation_trash",
    "object_create",
    "object_rename",
    "object_set_event",
    "object_set_geo",
    "map_file_set",
    "map_file_read",
    "object_set_prop",
    "object_move",
    "object_trash",
    "object_restore",
    "object_add_type",
    "object_remove_type",
    "blocks_list",
    "blocks_apply",
    "attachments_list",
    "attachment_add",
    "blob_read",
    "mirror_status",
    "mirror_enable",
    "mirror_disable",
    "mirror_sync_now",
    "conflicts_list",
    "conflict_resolve",
    "mirror_hold_decide",
    "export_run",
    "import_plan",
    "import_apply",
    "provenance_list",
    "settings_get",
    "settings_set",
    "app_diagnostics",
];

/// The Windows application manifest. tauri-build links its copy into the app binary only, so the
/// test binaries would start without Common Controls v6 and fail with STATUS_ENTRYPOINT_NOT_FOUND.
/// On MSVC targets the linker embeds this file into every binary instead.
const WINDOWS_MANIFEST: &str = "windows-app-manifest.xml";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    let mut attributes = tauri_build::Attributes::new().app_manifest(manifest);
    let target_os = std::env::var("CARGO_CFG_TARGET_OS")?;
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV")?;
    if target_os == "windows" && target_env == "msvc" {
        let path = std::env::current_dir()?.join(WINDOWS_MANIFEST);
        println!("cargo:rerun-if-changed={WINDOWS_MANIFEST}");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", path.display());
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }
    tauri_build::try_build(attributes)?;
    Ok(())
}
