// SPDX-License-Identifier: AGPL-3.0-or-later
//! The map file (M8, ADR-0033): a PMTiles archive on this machine that draws over the map the app
//! ships with. The webview cannot name a file to read here: it sets one, which has to be a PMTiles
//! archive, and then reads ranges of whichever is set. That keeps this from being a way to read any
//! file on the machine, and the check runs again on each read, since the file can change under it.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::settings::{self, Settings};
use crate::{IpcError, IpcErrorKind, Reply, Session};

/// How an archive of the format's third version begins.
const HEAD: &[u8; 8] = b"PMTiles\x03";

/// The most one read returns. A tile or a directory is far smaller, and a reply is held in memory
/// twice over on its way across.
pub const MAX_MAP_READ: u32 = 8 * 1024 * 1024;

fn opened(path: &str) -> Reply<File> {
    let mut file = File::open(path).map_err(|error| {
        IpcError::new(
            IpcErrorKind::BadPath,
            format!("the map file {path} cannot be opened: {error}"),
        )
    })?;
    let mut head = [0_u8; HEAD.len()];
    if file.read_exact(&mut head).is_err() || &head != HEAD {
        return Err(IpcError::new(
            IpcErrorKind::BadPath,
            format!("{path} is not a PMTiles version 3 file"),
        ));
    }
    Ok(file)
}

/// Chooses the map file, or with `None` goes back to the map the app ships with. Returns the settings
/// as they are now.
pub fn map_file_set(session: &Session, path: Option<String>) -> Reply<Settings> {
    if let Some(path) = &path {
        opened(path)?;
    }
    settings::update(session, |settings| Settings {
        map_file: path,
        ..settings
    })
}

/// Up to `length` bytes of the map file from `offset`, as base64: fewer at its end, and none past it.
pub fn map_file_read(session: &Session, offset: u64, length: u32) -> Reply<String> {
    if length > MAX_MAP_READ {
        return Err(IpcError::new(
            IpcErrorKind::Rule,
            format!("a read of the map file takes at most {MAX_MAP_READ} bytes"),
        ));
    }
    let settings = settings::settings_get(session)?;
    let Some(path) = settings.map_file else {
        return Err(IpcError::new(
            IpcErrorKind::NotFound,
            "no map file is chosen".to_owned(),
        ));
    };
    let mut file = opened(&path)?;
    let storage = |error: std::io::Error| {
        IpcError::new(
            IpcErrorKind::Storage,
            format!("reading the map file {path}: {error}"),
        )
    };
    file.seek(SeekFrom::Start(offset)).map_err(storage)?;
    let mut bytes = Vec::new();
    file.take(u64::from(length))
        .read_to_end(&mut bytes)
        .map_err(storage)?;
    Ok(BASE64.encode(bytes))
}
