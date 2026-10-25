// SPDX-License-Identifier: AGPL-3.0-or-later
//! Opening and creating vaults, transactions, and undo and redo.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use dbme_core::op::{Op, Row};
use dbme_core::seed::system_type_rows;
use dbme_core::{Clock, Id, IdGenerator};

use crate::blobs::BlobStore;
use crate::identity::Identity;
use crate::query::Reader;
use crate::txn::Txn;
use crate::{Result, StoreError, log, migrate, rows};

/// The database file inside a vault folder.
pub const DATABASE_FILE: &str = "vault.sqlite";
/// The file a running app holds a lock on, so only one process writes a vault at a time.
pub const LOCK_FILE: &str = "vault.lock";
/// The blob store folder.
pub const BLOBS_FOLDER: &str = "blobs";
/// The copy of the format specification every vault carries.
pub const FORMAT_FILE: &str = "FORMAT.md";
/// The extension of vault folders.
pub const VAULT_EXTENSION: &str = "dbme";

const FORMAT_TEXT: &str = include_str!("../../../docs/FORMAT.md");

/// How many undo steps a session keeps.
const UNDO_LIMIT: usize = 10_000;

/// What a vault needs from the app that opens it.
#[derive(Clone, Debug)]
pub struct VaultOptions {
    /// Who makes the changes, recorded in every op.
    pub identity: Identity,
    /// The time source for timestamps and IDs.
    pub clock: Arc<dyn Clock>,
}

pub(crate) struct Context {
    pub(crate) ids: IdGenerator,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) identity: Identity,
    pub(crate) blobs: BlobStore,
}

/// What a committed transaction, undo or redo did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Committed {
    /// The undo step: shared by all its ops.
    pub group_id: Id,
    /// For an undo or redo, the step it reversed.
    pub undo_of: Option<Id>,
    /// The `seq` of its first op.
    pub first_seq: i64,
    /// The `seq` of its last op.
    pub last_seq: i64,
    /// The IDs its ops targeted (for object types and attachments, the object's), sorted.
    pub changed: Vec<Id>,
}

#[derive(Clone, Copy, Debug)]
struct Step {
    group_id: Id,
    first_seq: i64,
    last_seq: i64,
}

impl From<&Committed> for Step {
    fn from(committed: &Committed) -> Self {
        Self {
            group_id: committed.group_id,
            first_seq: committed.first_seq,
            last_seq: committed.last_seq,
        }
    }
}

/// An open vault. Only one process at a time can hold a vault open.
pub struct Vault {
    // Fields drop in order: the database closes before the lock is released.
    conn: Connection,
    ctx: Context,
    root: PathBuf,
    vault_id: Id,
    undo: Vec<Step>,
    redo: Vec<Step>,
    _lock: File,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.root)
            .field("vault_id", &self.vault_id)
            .finish_non_exhaustive()
    }
}

fn check_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    match name.strip_suffix(".dbme") {
        Some(stem) if !stem.is_empty() && !stem.starts_with('.') => Ok(name.to_owned()),
        _ => Err(StoreError::BadVaultName(path.to_owned())),
    }
}

fn lock(dir: &Path) -> Result<File> {
    let path = dir.join(LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| StoreError::io("opening", &path, e))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(StoreError::Locked(dir.to_owned())),
        Err(TryLockError::Error(e)) => Err(StoreError::io("locking", path, e)),
    }
}

impl Vault {
    /// Creates a vault at `path`, a folder named `<name>.dbme` that must not exist yet (or be
    /// empty), and opens it. The vault is built in a hidden folder next to it and renamed into
    /// place when complete, so a failed creation leaves nothing behind.
    pub fn create(path: &Path, options: VaultOptions) -> Result<Self> {
        let name = check_name(path)?;
        if path.exists() {
            let empty = fs::read_dir(path)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if !empty {
                return Err(StoreError::AlreadyExists(path.to_owned()));
            }
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let staging = parent.join(format!(
            ".{name}.creating-{}",
            IdGenerator::new().next(options.clock.now_ms())?
        ));
        fs::create_dir(&staging).map_err(|e| StoreError::io("creating", &staging, e))?;
        let built = Self::build(&staging, &options).and_then(|()| {
            if path.exists() {
                fs::remove_dir(path).map_err(|e| StoreError::io("replacing", path, e))?;
            }
            fs::rename(&staging, path).map_err(|e| StoreError::io("creating", path, e))
        });
        if let Err(error) = built {
            // Only the staging folder, which this call made, is removed.
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        Self::open(path, options)
    }

    fn build(dir: &Path, options: &VaultOptions) -> Result<()> {
        let blobs = dir.join(BLOBS_FOLDER);
        fs::create_dir(&blobs).map_err(|e| StoreError::io("creating", &blobs, e))?;
        let format = dir.join(FORMAT_FILE);
        fs::write(&format, FORMAT_TEXT).map_err(|e| StoreError::io("writing", &format, e))?;
        let mut vault = Self::open_folder(dir, options.clone(), true)?;
        vault.transact(|txn| {
            for row in system_type_rows(txn.now(), txn.ids())? {
                txn.apply(Op::create(Row::Type(row)))?;
            }
            Ok(())
        })?;
        // The built-in types are where a vault starts, not a step to undo.
        vault.undo.clear();
        Ok(())
    }

    /// Opens the vault at `path`.
    pub fn open(path: &Path, options: VaultOptions) -> Result<Self> {
        check_name(path)?;
        Self::open_folder(path, options, false)
    }

    fn open_folder(dir: &Path, options: VaultOptions, create: bool) -> Result<Self> {
        let database = dir.join(DATABASE_FILE);
        if !create && !database.is_file() {
            return Err(StoreError::NotAVault(dir.to_owned()));
        }
        let lock = lock(dir)?;
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI;
        let flags = if create {
            flags | OpenFlags::SQLITE_OPEN_CREATE
        } else {
            flags
        };
        let mut conn = Connection::open_with_flags(&database, flags)?;
        migrate::migrate(&mut conn, dir)?;
        migrate::configure(&conn)?;
        let now = options.clock.now_ms();
        let ids = IdGenerator::new();
        let stored: Option<String> = conn
            .query_row(
                "SELECT value FROM vault_meta WHERE key = 'vault_id'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let vault_id = match stored {
            Some(text) => Id::parse(&text)?,
            None if create => {
                let id = ids.next(now)?;
                let meta = [
                    ("vault_id", id.to_string()),
                    ("created_at", now.to_string()),
                    (
                        "created_by",
                        format!("{} {}", dbme_core::PRODUCT_NAME, dbme_core::VERSION),
                    ),
                    ("format_version", dbme_core::FORMAT_VERSION.to_string()),
                ];
                let tx = conn.transaction()?;
                for (key, value) in meta {
                    tx.execute(
                        "INSERT INTO vault_meta (key, value) VALUES (?1, ?2)",
                        params![key, value],
                    )?;
                }
                tx.commit()?;
                id
            }
            None => return Err(StoreError::NotAVault(dir.to_owned())),
        };
        let ctx = Context {
            ids,
            clock: options.clock,
            identity: options.identity,
            blobs: BlobStore::new(dir.join(BLOBS_FOLDER)),
        };
        Ok(Self {
            conn,
            ctx,
            root: dir.to_owned(),
            vault_id,
            undo: Vec::new(),
            redo: Vec::new(),
            _lock: lock,
        })
    }

    /// The vault folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The vault's own ID, fixed when it was created.
    pub fn id(&self) -> Id {
        self.vault_id
    }

    /// The vault's blob store.
    pub fn blobs(&self) -> &BlobStore {
        &self.ctx.blobs
    }

    /// Reads the vault.
    pub fn read(&self) -> Reader<'_> {
        Reader { conn: &self.conn }
    }

    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Runs `change` in one transaction. If it returns an error, nothing is saved. Otherwise the
    /// changes commit together as one undo step, and the redo history is cleared. The second
    /// value is `None` if nothing changed.
    pub fn transact<T>(
        &mut self,
        change: impl FnOnce(&mut Txn<'_>) -> Result<T>,
    ) -> Result<(T, Option<Committed>)> {
        let mut txn = Txn::begin(&mut self.conn, &self.ctx, None)?;
        let value = change(&mut txn)?;
        let committed = txn.finish()?;
        if let Some(committed) = &committed {
            if self.undo.len() == UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.undo.push(committed.into());
            self.redo.clear();
        }
        Ok((value, committed))
    }

    /// Whether there is a step to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is an undone step to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the last step of this session. Returns `None` if there is none.
    ///
    /// If anything that step changed has changed again since, in a way the undo stack does not
    /// cover, the undo is refused with [`StoreError::UndoBlocked`], nothing changes, and the step
    /// is dropped from the stack so the steps before it can still be undone.
    pub fn undo(&mut self) -> Result<Option<Committed>> {
        let Some(step) = self.undo.pop() else {
            return Ok(None);
        };
        let committed = self.reverse(step)?;
        self.redo.push((&committed).into());
        Ok(Some(committed))
    }

    /// Redoes the last undone step. Returns `None` if there is none. Refused like [`Vault::undo`].
    pub fn redo(&mut self) -> Result<Option<Committed>> {
        let Some(step) = self.redo.pop() else {
            return Ok(None);
        };
        let committed = self.reverse(step)?;
        self.undo.push((&committed).into());
        Ok(Some(committed))
    }

    /// Applies the inverse of every op of `step`, last first, as a new step.
    fn reverse(&mut self, step: Step) -> Result<Committed> {
        let mut txn = Txn::begin(&mut self.conn, &self.ctx, Some(step.group_id))?;
        let mut ops = Vec::new();
        log::for_each(
            txn.read().conn,
            step.first_seq,
            step.last_seq,
            true,
            |record| {
                ops.push(record.entry.op()?);
                Ok(())
            },
        )?;
        for op in ops {
            let current = rows::load(txn.read().conn, &op.key())?;
            let inverse = op
                .inverse(current.as_ref())
                .map_err(|e| StoreError::from(e).blocking_undo())?;
            txn.apply_to(inverse, current.as_ref())
                .map_err(StoreError::blocking_undo)?;
        }
        txn.finish()?
            .ok_or_else(|| StoreError::UndoBlocked("the step has no changes".to_owned()))
    }
}
