// SPDX-License-Identifier: AGPL-3.0-or-later
//! The editable system types every new vault starts with (handoff brief §4, FORMAT.md §3.2).

use crate::model::TypeRow;
use crate::{CoreError, IdGenerator};

/// A built-in type: its name, plural, icon (a Lucide icon name) and the type it extends.
#[derive(Clone, Copy, Debug)]
pub struct SystemType {
    /// Singular name.
    pub name: &'static str,
    /// Plural name.
    pub plural: &'static str,
    /// Icon name.
    pub icon: &'static str,
    /// Name of the type it extends, if any. It always appears earlier in [`SYSTEM_TYPES`].
    pub extends: Option<&'static str>,
}

const fn system(name: &'static str, plural: &'static str, icon: &'static str) -> SystemType {
    SystemType {
        name,
        plural,
        icon,
        extends: None,
    }
}

const fn media(name: &'static str, plural: &'static str, icon: &'static str) -> SystemType {
    SystemType {
        name,
        plural,
        icon,
        extends: Some("Media"),
    }
}

/// The 25 system types, parents before the types that extend them.
pub const SYSTEM_TYPES: [SystemType; 25] = [
    system("Note", "Notes", "sticky-note"),
    system("Page", "Pages", "file"),
    system("Task", "Tasks", "square-check"),
    system("Person", "People", "user"),
    system("Organization", "Organizations", "building-2"),
    system("Place", "Places", "map-pin"),
    system("Event", "Events", "calendar"),
    system("Project", "Projects", "folder-kanban"),
    system("Area", "Areas", "layers"),
    system("Resource", "Resources", "book-open"),
    system("Media", "Media", "clapperboard"),
    system("WebClip", "Web clips", "globe"),
    system("Document", "Documents", "file-text"),
    media("Image", "Images", "image"),
    media("Audio", "Audio", "audio-lines"),
    media("Video", "Videos", "video"),
    system("Email", "Emails", "mail"),
    system("Message", "Messages", "message-square"),
    system("Contact", "Contacts", "contact"),
    system("Dataset", "Datasets", "table"),
    system("Tag", "Tags", "tag"),
    system("Collection", "Collections", "library"),
    system("Template", "Templates", "layout-template"),
    system("Highlight", "Highlights", "highlighter"),
    system("Journal", "Journals", "notebook-pen"),
];

/// Rows for the system types, created at time `ts` with fresh IDs.
pub fn system_type_rows(ts: i64, ids: &IdGenerator) -> Result<Vec<TypeRow>, CoreError> {
    let mut rows: Vec<TypeRow> = Vec::with_capacity(SYSTEM_TYPES.len());
    for system in SYSTEM_TYPES {
        let extends = match system.extends {
            Some(parent) => Some(
                rows.iter()
                    .find(|row| row.name == parent)
                    .map(|row| row.id)
                    .ok_or_else(|| {
                        CoreError::invalid(
                            "system type",
                            format!("{parent} must come before {}", system.name),
                        )
                    })?,
            ),
            None => None,
        };
        rows.push(TypeRow {
            id: ids.next(ts)?,
            name: system.name.to_owned(),
            plural: Some(system.plural.to_owned()),
            icon: Some(system.icon.to_owned()),
            color: None,
            extends,
            description: None,
            is_system: true,
            created_at: ts,
            updated_at: ts,
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brief_s_types_are_all_there_and_valid() {
        let rows = system_type_rows(1, &IdGenerator::new()).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(
            names.join(", "),
            "Note, Page, Task, Person, Organization, Place, Event, Project, Area, Resource, Media, WebClip, Document, Image, Audio, Video, Email, Message, Contact, Dataset, Tag, Collection, Template, Highlight, Journal"
        );
        for row in &rows {
            row.validate().unwrap();
            assert!(row.is_system);
        }
        let media = rows.iter().find(|r| r.name == "Media").unwrap().id;
        for name in ["Image", "Audio", "Video"] {
            assert_eq!(
                rows.iter().find(|r| r.name == name).unwrap().extends,
                Some(media)
            );
        }
    }
}
