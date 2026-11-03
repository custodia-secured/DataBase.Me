// SPDX-License-Identifier: AGPL-3.0-or-later
//! Runs a command by name, with its arguments as the UI sends them (camelCase keys), exactly as the
//! desktop shell's wrapper would. A test in the desktop crate checks that every command in its
//! `build.rs` has an arm here.

use std::path::Path;
use std::sync::Arc;

use dbme_api::{self as api, IpcError, IpcErrorKind, Reply};
use dbme_core::SystemClock;
use dbme_store::{Identity, VaultOptions};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::TestSession;

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorKind::Invalid, message)
}

/// Argument `name` of `args`; a missing argument reads as `null`, which is how an absent option
/// arrives from the UI.
fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Reply<T> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|e| invalid(format!("argument {name}: {e}")))
}

fn ok<T: Serialize>(reply: Reply<T>) -> Reply<Value> {
    reply.and_then(|value| {
        serde_json::to_value(value)
            .map_err(|e| IpcError::new(IpcErrorKind::Storage, format!("cannot encode: {e}")))
    })
}

fn options(session: &TestSession) -> Reply<VaultOptions> {
    let clock = Arc::new(SystemClock);
    let identity = Identity::load_or_create(&session.dir.join("config"), clock.as_ref())?;
    Ok(VaultOptions { identity, clock })
}

/// A vault path the UI sent, which must be inside the session's own folder.
fn vault_path(session: &TestSession, path: &str) -> Reply<String> {
    let inside = Path::new(path).starts_with(&session.dir)
        && !Path::new(path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
    if inside {
        Ok(path.to_owned())
    } else {
        Err(IpcError::new(
            IpcErrorKind::BadPath,
            "the test server only opens vaults in the session's folder",
        ))
    }
}

/// Runs `cmd` for `session`.
pub(crate) fn dispatch(session: &TestSession, cmd: &str, args: &Value) -> Reply<Value> {
    let s = &session.api;
    match cmd {
        "app_info" => Ok(json!({
            "product": dbme_core::PRODUCT_NAME,
            "version": dbme_core::VERSION,
            "format_version": dbme_core::FORMAT_VERSION,
            "smoke_vault": null,
        })),
        "app_ready" => Ok(Value::Null),
        "vault_create" => {
            let path = vault_path(session, &arg::<String>(args, "path")?)?;
            ok(api::vault_create(s, &path, options(session)?))
        }
        "vault_open" => {
            let path = vault_path(session, &arg::<String>(args, "path")?)?;
            ok(api::vault_open(s, &path, options(session)?))
        }
        "vault_open_default" => {
            let path = session.default_vault();
            let path = path.display().to_string();
            if Path::new(&path).exists() {
                ok(api::vault_open(s, &path, options(session)?))
            } else {
                ok(api::vault_create(s, &path, options(session)?))
            }
        }
        "vault_close" => ok(api::vault_close(s)),
        "vault_info" => ok(api::vault_info(s)),
        "undo" => ok(api::undo(s)),
        "redo" => ok(api::redo(s)),
        "types_list" => ok(api::types_list(s)),
        "fields_list" => ok(api::fields_list(s, arg(args, "source")?)),
        "field_create" => ok(api::field_create(s, arg(args, "field")?)),
        "object_get" => ok(api::object_get(s, arg(args, "id")?)),
        "object_children" => ok(api::object_children(s, arg(args, "parent")?)),
        "object_ancestors" => ok(api::object_ancestors(s, arg(args, "id")?)),
        "object_summaries" => ok(api::object_summaries(s, &arg::<Vec<_>>(args, "ids")?)),
        "objects_page" => ok(api::objects_page(
            s,
            arg(args, "source")?,
            &arg(args, "sort")?,
            arg(args, "offset")?,
            arg(args, "limit")?,
        )),
        "objects_count" => ok(api::objects_count(s, arg(args, "source")?)),
        "object_search" => ok(api::object_search(
            s,
            &arg::<String>(args, "text")?,
            arg(args, "limit")?,
        )),
        "backlinks" => ok(api::backlinks(s, arg(args, "id")?)),
        "object_tree" => ok(api::object_tree(s, arg(args, "root")?, arg(args, "limit")?)),
        "search" => ok(api::search(
            s,
            &arg::<String>(args, "text")?,
            arg(args, "limit")?,
        )),
        "objects_query" => ok(api::objects_query(s, &arg(args, "query")?)),
        "objects_query_count" => ok(api::objects_query_count(s, &arg(args, "query")?)),
        "facets" => ok(api::facets(
            s,
            &arg(args, "query")?,
            &arg::<String>(args, "field")?,
        )),
        "views_list" => ok(api::views_list(s)),
        "view_get" => ok(api::view_get(s, arg(args, "id")?)),
        "view_save" => ok(api::view_save(s, arg(args, "view")?)),
        "view_update" => ok(api::view_update(s, arg(args, "id")?, arg(args, "view")?)),
        "view_delete" => ok(api::view_delete(s, arg(args, "id")?)),
        "relation_create" => ok(api::relation_create(
            s,
            arg(args, "src")?,
            arg(args, "dst")?,
        )),
        "relation_trash" => ok(api::relation_trash(s, arg(args, "id")?)),
        "object_create" => ok(api::object_create(s, arg(args, "object")?)),
        "object_rename" => ok(api::object_rename(s, arg(args, "id")?, arg(args, "title")?)),
        "object_set_prop" => ok(api::object_set_prop(
            s,
            arg(args, "id")?,
            arg(args, "key")?,
            arg(args, "value")?,
        )),
        "object_move" => ok(api::object_move(
            s,
            arg(args, "id")?,
            arg(args, "parent")?,
            arg(args, "position")?,
        )),
        "object_trash" => ok(api::object_trash(s, arg(args, "id")?)),
        "object_restore" => ok(api::object_restore(s, arg(args, "id")?)),
        "object_add_type" => ok(api::object_add_type(
            s,
            arg(args, "id")?,
            arg(args, "typeId")?,
        )),
        "object_remove_type" => ok(api::object_remove_type(
            s,
            arg(args, "id")?,
            arg(args, "typeId")?,
        )),
        "blocks_list" => ok(api::blocks_list(s, arg(args, "objectId")?)),
        "blocks_apply" => ok(api::blocks_apply(
            s,
            arg(args, "objectId")?,
            arg(args, "changes")?,
        )),
        "attachments_list" => ok(api::attachments_list(s, arg(args, "objectId")?)),
        "attachment_add" => ok(api::attachment_add(
            s,
            arg(args, "objectId")?,
            &arg::<String>(args, "data")?,
            arg(args, "filename")?,
            arg(args, "mime")?,
        )),
        "blob_read" => ok(api::blob_read(s, &arg(args, "hash")?)),
        "mirror_status" => ok(api::mirror_status(s)),
        "mirror_enable" => {
            let folder: Option<String> = arg(args, "folder")?;
            let folder = folder.map(|path| vault_path(session, &path)).transpose()?;
            ok(api::mirror_enable(s, folder))
        }
        "mirror_disable" => ok(api::mirror_disable(s)),
        "mirror_sync_now" => ok(api::mirror_sync_now(s)),
        "conflicts_list" => ok(api::conflicts_list(s, arg(args, "openOnly")?)),
        "conflict_resolve" => ok(api::conflict_resolve(
            s,
            arg(args, "id")?,
            arg(args, "resolution")?,
        )),
        "mirror_hold_decide" => ok(api::mirror_hold_decide(
            s,
            arg(args, "id")?,
            arg(args, "choice")?,
        )),
        "export_run" => {
            let dir = vault_path(session, &arg::<String>(args, "dir")?)?;
            ok(api::export_run(s, dir, arg(args, "full")?))
        }
        "import_plan" => {
            let sources: Vec<String> = arg(args, "sources")?;
            let sources = sources
                .iter()
                .map(|path| vault_path(session, path))
                .collect::<Reply<Vec<String>>>()?;
            ok(api::import_plan(s, sources, arg(args, "options")?))
        }
        "import_apply" => ok(api::import_apply(s, arg(args, "plan")?)),
        "provenance_list" => ok(api::provenance_list(s, arg(args, "objectId")?)),
        _ => Err(invalid(format!("unknown command {cmd:?}"))),
    }
}
