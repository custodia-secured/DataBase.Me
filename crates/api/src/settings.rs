// SPDX-License-Identifier: AGPL-3.0-or-later
//! The person's settings (M7, ADR-0027): the mode, the theme, whether update checks are wanted,
//! the last vault and whether onboarding ran. They are the person's, not the vault's, so they
//! live in one `settings.json` in the app's config folder, beside the identity and the files
//! folder's override file, and a vault carries nothing about anyone's screen. The file is
//! versioned; a missing key takes its default, so an older file always loads, and a file that
//! cannot be read is left alone and read as the defaults until the next write replaces it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{IpcError, IpcErrorKind, Reply, Session};

/// The settings file's name, in the app's config folder.
pub const SETTINGS_FILE: &str = "settings.json";

/// The version this app writes. A later app that changes a key's meaning bumps it and reads
/// older files accordingly.
pub const SETTINGS_VERSION: u32 = 1;

/// A preset of what is shown (brief §2.9): Simple reads as a plain notes app and hides types,
/// fields and the Table view; Organizer shows everything. Researcher and Data/Power come with
/// later milestones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Mode {
    /// A plain notes app: pages, search, the mind map, the gallery, the files folder, import.
    #[default]
    Simple,
    /// Everything of M1 to M6: types, fields, the Table view, tags.
    Organizer,
}

/// The colours: the OS's choice, or light or dark regardless.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Theme {
    /// Whatever the OS asks for.
    #[default]
    System,
    /// Light colours regardless of the OS.
    Light,
    /// Dark colours regardless of the OS.
    Dark,
}

/// Everything the app remembers about the person's choices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Settings {
    /// The file's version, [`SETTINGS_VERSION`] when this app wrote it.
    pub version: u32,
    /// Which preset of features is shown.
    pub mode: Mode,
    /// The colours.
    pub theme: Theme,
    /// Whether the person agreed to update checks. Off by default (brief §2.2); 0.1 has no
    /// updater, so the choice is recorded for the first version that does, and neither answer
    /// causes a request (ADR-0010).
    pub update_checks: bool,
    /// The vault opened last, to open again at the next start.
    pub last_vault: Option<String>,
    /// Whether onboarding has run to its end.
    pub onboarded: bool,
    /// The PMTiles file drawn on maps, if one was chosen (`map_file_set`).
    pub map_file: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            mode: Mode::default(),
            theme: Theme::default(),
            update_checks: false,
            last_vault: None,
            onboarded: false,
            map_file: None,
        }
    }
}

/// What a change sets: every key is optional, and only the keys given change.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SettingsPatch {
    /// A new mode.
    pub mode: Option<Mode>,
    /// A new theme.
    pub theme: Option<Theme>,
    /// Whether update checks are wanted.
    pub update_checks: Option<bool>,
    /// The vault to open next time.
    pub last_vault: Option<String>,
    /// Whether onboarding ran to its end.
    pub onboarded: Option<bool>,
}

impl Settings {
    /// These settings with `patch` applied.
    #[must_use]
    pub fn with(mut self, patch: SettingsPatch) -> Self {
        if let Some(mode) = patch.mode {
            self.mode = mode;
        }
        if let Some(theme) = patch.theme {
            self.theme = theme;
        }
        if let Some(update_checks) = patch.update_checks {
            self.update_checks = update_checks;
        }
        if let Some(last_vault) = patch.last_vault {
            self.last_vault = Some(last_vault);
        }
        if let Some(onboarded) = patch.onboarded {
            self.onboarded = onboarded;
        }
        self.version = SETTINGS_VERSION;
        self
    }
}

fn file_in(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

/// The settings in `config_dir`: the defaults when there is no folder or no file, or when the
/// file cannot be read, which is logged and never fatal.
pub(crate) fn load(config_dir: Option<&Path>) -> Settings {
    let Some(dir) = config_dir else {
        return Settings::default();
    };
    let path = file_in(dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Settings::default(),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "settings could not be read; using the defaults");
            return Settings::default();
        }
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "settings are not valid; using the defaults");
            Settings::default()
        }
    }
}

/// Writes `settings` to `config_dir` whole, through a temporary file and a rename, so a crash
/// leaves the old file or the new one and never half of either.
pub(crate) fn save(config_dir: &Path, settings: &Settings) -> Reply<()> {
    let storage = |what: &str, error: std::io::Error| {
        IpcError::new(
            IpcErrorKind::Storage,
            format!("{what} {}: {error}", file_in(config_dir).display()),
        )
    };
    fs::create_dir_all(config_dir).map_err(|e| storage("making the folder of", e))?;
    let text = serde_json::to_string_pretty(settings)
        .map_err(|e| IpcError::new(IpcErrorKind::Storage, format!("encoding settings: {e}")))?;
    let path = file_in(config_dir);
    let temp = config_dir.join(format!("{SETTINGS_FILE}.tmp"));
    fs::write(&temp, format!("{text}\n")).map_err(|e| storage("writing", e))?;
    fs::rename(&temp, &path).map_err(|e| storage("replacing", e))?;
    Ok(())
}

/// The person's settings.
pub fn settings_get(session: &Session) -> Reply<Settings> {
    Ok(session.settings())
}

/// Changes the keys `patch` gives, writes the file and returns the settings as they are now.
pub fn settings_set(session: &Session, patch: SettingsPatch) -> Reply<Settings> {
    update(session, |settings| settings.with(patch))
}

/// Changes the settings as `change` says, writes the file and returns them as they are now.
pub(crate) fn update(
    session: &Session,
    change: impl FnOnce(Settings) -> Settings,
) -> Reply<Settings> {
    let mut guard = session.lock_settings();
    let next = change(
        guard
            .clone()
            .unwrap_or_else(|| load(session.config_dir.as_deref())),
    );
    if let Some(dir) = session.config_dir.as_deref() {
        save(dir, &next)?;
    }
    *guard = Some(next.clone());
    Ok(next)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::*;

    #[test]
    fn defaults_without_a_folder_and_changes_kept_in_memory() {
        let session = Session::new(|_| {});
        assert_eq!(settings_get(&session).unwrap(), Settings::default());
        let after = settings_set(
            &session,
            SettingsPatch {
                mode: Some(Mode::Organizer),
                ..SettingsPatch::default()
            },
        )
        .unwrap();
        assert_eq!(after.mode, Mode::Organizer);
        assert_eq!(settings_get(&session).unwrap(), after);
    }

    #[test]
    fn the_file_round_trips_and_an_older_or_broken_file_reads_as_defaults_plus_what_it_has() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let session = Session::new(|_| {}).with_config_dir(&config);
        assert_eq!(settings_get(&session).unwrap(), Settings::default());
        let set = settings_set(
            &session,
            SettingsPatch {
                theme: Some(Theme::Dark),
                update_checks: Some(true),
                last_vault: Some("C:/vaults/Main.dbme".into()),
                onboarded: Some(true),
                ..SettingsPatch::default()
            },
        )
        .unwrap();
        let text = fs::read_to_string(config.join(SETTINGS_FILE)).unwrap();
        assert!(text.contains("\"version\": 1") && text.contains("\"theme\": \"dark\""));
        assert!(!config.join(format!("{SETTINGS_FILE}.tmp")).exists());

        // Another session over the same folder reads the file.
        let again = Session::new(|_| {}).with_config_dir(&config);
        assert_eq!(settings_get(&again).unwrap(), set);

        // An older file with a key missing and one unknown: the known keys, defaults for the rest.
        fs::write(
            config.join(SETTINGS_FILE),
            r#"{"version": 0, "mode": "organizer", "colour": "mauve"}"#,
        )
        .unwrap();
        let older = Session::new(|_| {}).with_config_dir(&config);
        let read = settings_get(&older).unwrap();
        assert_eq!(
            (read.mode, read.theme, read.version),
            (Mode::Organizer, Theme::System, 0)
        );
        // A write brings the version up and keeps what was read.
        let written = settings_set(&older, SettingsPatch::default()).unwrap();
        assert_eq!(
            (written.mode, written.version),
            (Mode::Organizer, SETTINGS_VERSION)
        );

        // A broken file is the defaults until the next write replaces it.
        fs::write(config.join(SETTINGS_FILE), "{not json").unwrap();
        let broken = Session::new(|_| {}).with_config_dir(&config);
        assert_eq!(settings_get(&broken).unwrap(), Settings::default());
        assert_eq!(
            fs::read_to_string(config.join(SETTINGS_FILE)).unwrap(),
            "{not json",
            "a read never writes"
        );
        settings_set(&broken, SettingsPatch::default()).unwrap();
        assert_eq!(
            settings_get(&Session::new(|_| {}).with_config_dir(&config)).unwrap(),
            Settings::default()
        );
    }
}
