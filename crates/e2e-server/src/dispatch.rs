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
        _ => Err(invalid(format!("unknown command {cmd:?}"))),
    }
}
