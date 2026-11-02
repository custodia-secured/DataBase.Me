// SPDX-License-Identifier: AGPL-3.0-or-later
//! The platform watcher (ADR-0024): `notify` 8 with `notify-debouncer-full`, turned into
//! [`FileEvent`]s by the rules the Windows leg of gate 1e verified. The engine never depends on
//! it for correctness: [`crate::Mirror::rescan`] derives the same events from the folder itself,
//! so this module is excluded from the coverage gate and tested as a probe on each OS.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use notify_debouncer_full::notify::event::{CreateKind, ModifyKind, RenameMode};
use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache};

use crate::engine::{FileEvent, Watcher};
use crate::{MirrorError, Result};

/// The debounce window the app owns (M5.md: one debounce, 300 to 500 ms).
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(400);

/// A recursive watch on the files folder.
pub struct NotifyWatcher {
    root: PathBuf,
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    rx: Receiver<DebounceEventResult>,
    errors: Vec<String>,
}

impl std::fmt::Debug for NotifyWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotifyWatcher")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl NotifyWatcher {
    /// Starts watching `root` recursively, reporting after `debounce` of quiet.
    pub fn new(root: &Path, debounce: Duration) -> Result<Self> {
        let root = root
            .canonicalize()
            .map_err(|e| MirrorError::io("watching", &root.display().to_string(), e))?;
        let (tx, rx) = mpsc::channel();
        let mut debouncer = notify_debouncer_full::new_debouncer(debounce, None, tx)
            .map_err(|e| notify_error("watching", &root, e))?;
        debouncer
            .watch(&root, RecursiveMode::Recursive)
            .map_err(|e| notify_error("watching", &root, e))?;
        Ok(Self {
            root,
            _debouncer: debouncer,
            rx,
            errors: Vec::new(),
        })
    }

    /// The folder being watched, canonical.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Errors the backend reported since the last call, as text. The engine keeps going; a
    /// caller that sees one should schedule a rescan.
    pub fn take_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.errors)
    }

    /// Blocks up to `timeout` for the next batch, then returns everything available.
    pub fn poll_timeout(&mut self, timeout: Duration) -> Vec<FileEvent> {
        let mut out = Vec::new();
        if let Ok(batch) = self.rx.recv_timeout(timeout) {
            self.take(batch, &mut out);
        }
        self.drain(&mut out);
        out
    }

    fn drain(&mut self, out: &mut Vec<FileEvent>) {
        loop {
            match self.rx.try_recv() {
                Ok(batch) => self.take(batch, out),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }

    fn take(&mut self, batch: DebounceEventResult, out: &mut Vec<FileEvent>) {
        match batch {
            Ok(events) => {
                for event in events {
                    out.extend(self.convert(&event));
                }
            }
            Err(errors) => self.errors.extend(errors.iter().map(ToString::to_string)),
        }
    }

    fn relative(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let text = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        (!text.is_empty()).then_some(text)
    }

    /// One debounced event as the engine's events (ADR-0024).
    fn convert(&self, event: &DebouncedEvent) -> Vec<FileEvent> {
        let paths: Vec<(String, &Path)> = event
            .paths
            .iter()
            .filter_map(|p| self.relative(p).map(|rel| (rel, p.as_path())))
            .collect();
        let is_dir = |p: &Path| p.is_dir();
        match &event.kind {
            EventKind::Create(kind) => paths
                .into_iter()
                .map(|(rel, p)| {
                    if matches!(kind, CreateKind::Folder) || is_dir(p) {
                        FileEvent::DirCreated(rel)
                    } else {
                        FileEvent::Created(rel)
                    }
                })
                .collect(),
            EventKind::Remove(_) => paths
                .into_iter()
                .map(|(rel, _)| FileEvent::Removed(rel))
                .collect(),
            EventKind::Modify(ModifyKind::Name(mode)) => match (mode, paths.as_slice()) {
                (RenameMode::Both | RenameMode::Any, [(from, _), (to, _)]) => {
                    vec![FileEvent::Renamed {
                        from: from.clone(),
                        to: to.clone(),
                    }]
                }
                (RenameMode::From, [(rel, _)]) => vec![FileEvent::Removed(rel.clone())],
                (RenameMode::To, [(rel, p)]) => vec![if is_dir(p) {
                    FileEvent::DirCreated(rel.clone())
                } else {
                    FileEvent::Created(rel.clone())
                }],
                (_, one) => one
                    .iter()
                    .map(|(rel, p)| {
                        if p.exists() {
                            if is_dir(p) {
                                FileEvent::DirCreated(rel.clone())
                            } else {
                                FileEvent::Created(rel.clone())
                            }
                        } else {
                            FileEvent::Removed(rel.clone())
                        }
                    })
                    .collect(),
            },
            EventKind::Modify(_) => paths
                .into_iter()
                .filter(|(_, p)| !is_dir(p))
                .map(|(rel, _)| FileEvent::Modified(rel))
                .collect(),
            EventKind::Access(_) | EventKind::Other | EventKind::Any => Vec::new(),
        }
    }
}

impl Watcher for NotifyWatcher {
    fn poll(&mut self) -> Vec<FileEvent> {
        let mut out = Vec::new();
        self.drain(&mut out);
        out
    }
}

fn notify_error(
    action: &'static str,
    root: &Path,
    error: notify_debouncer_full::notify::Error,
) -> MirrorError {
    MirrorError::io(
        action,
        &root.display().to_string(),
        std::io::Error::other(error.to_string()),
    )
}
