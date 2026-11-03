// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Obsidian fixture (M5 acceptance: "Obsidian fixture opens with working links"). In: the
//! folder read into a fresh vault gives every link in `links.json` the relation, mention, embed or
//! attachment it names, the tags their Tag objects, and the cloud drives' copies the inbox. Out:
//! the vault written back has zero dangling wikilinks by the folder's own resolver, and the
//! written files equal the golden in `fixtures/obsidian-roundtrip/`, ids normalised.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers outside #[test] functions may panic"
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dbme_core::files::LinkResolver;
use dbme_core::{Id, ManualClock};
use dbme_mirror::{Mirror, StdFs, VaultResolver};
use dbme_store::{ConflictKind, Identity, Vault, VaultOptions};
use serde::Deserialize;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");
/// 2026-11-02T10:40:00Z, the clock every object's `updated` shows after the read.
const START: i64 = 1_793_616_000_000;

#[derive(Deserialize)]
struct Manifest {
    links: Vec<Link>,
    tags: BTreeMap<String, Vec<String>>,
    cloud_conflict_files: Vec<String>,
}

#[derive(Deserialize)]
struct Link {
    from_id: Option<Id>,
    link: String,
    to: String,
    to_id: Option<Id>,
    kind: String,
    shown: Option<String>,
    block: Option<Id>,
    label: Option<String>,
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Every file under `root`, by relative path with forward slashes, with its bytes.
fn files_under(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn is_ulid_char(c: u8) -> bool {
    matches!(c, b'0'..=b'9' | b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z')
}

/// Replaces every 26-character ULID with `ULID<n>`, numbered by first appearance.
fn normalise(text: &str, seen: &mut HashMap<String, usize>) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let run = bytes[i..].iter().take_while(|b| is_ulid_char(**b)).count();
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if run == 26 && before_ok {
            let id = &text[i..i + 26];
            let n = seen.len() + 1;
            let n = *seen.entry(id.to_owned()).or_insert(n);
            out.push_str(&format!("ULID{n}"));
            i += 26;
        } else if run > 0 {
            out.push_str(&text[i..i + run]);
            i += run;
        } else {
            let c = text[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// The wikilink targets in a Markdown text: `[[target]]`, `![[target|shown]]`, `[[target#^id]]`.
fn wikilinks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        let target = inner.split(['|', '#']).next().unwrap_or("");
        out.push(target.to_owned());
        rest = &after[end + 2..];
    }
    out
}

struct Read {
    _dir: tempfile::TempDir,
    folder: PathBuf,
    vault: Vault,
    mirror: Mirror<StdFs>,
    report: dbme_mirror::Inbound,
}

/// The fixture copied to a folder and read into a fresh vault.
fn read_fixture() -> Read {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("files");
    copy_tree(&Path::new(FIXTURES).join("obsidian"), &folder);
    // The fixture's own documentation is not part of the vault.
    for doc in ["README.md", "links.json"] {
        fs::remove_file(folder.join(doc)).unwrap();
    }
    let clock = Arc::new(ManualClock::new(START));
    let mut vault = Vault::create(
        &dir.path().join("Fresh.dbme"),
        VaultOptions {
            identity: Identity {
                actor_id: Id::from_parts(1, 1),
                device_id: Id::from_parts(1, 2),
            },
            clock: clock.clone(),
        },
    )
    .unwrap();
    let mut mirror = Mirror::new(StdFs::new(&folder).unwrap(), clock);
    let report = mirror.rebuild(&mut vault).unwrap();
    Read {
        _dir: dir,
        folder,
        vault,
        mirror,
        report,
    }
}

#[test]
fn every_link_in_the_manifest_arrives_as_it_says() {
    let r = read_fixture();
    let manifest: Manifest =
        serde_json::from_slice(&fs::read(Path::new(FIXTURES).join("obsidian/links.json")).unwrap())
            .unwrap();
    assert!(r.report.held.is_none(), "{:?}", r.report);
    let read = r.vault.read();

    for link in &manifest.links {
        let from = link.from_id.unwrap();
        let what = format!("{} in {}", link.link, from);
        match link.kind.as_str() {
            "mention" | "embed" => {
                let to = link.to_id.unwrap();
                let runs: Vec<serde_json::Value> = read
                    .blocks(from)
                    .unwrap()
                    .into_iter()
                    .flat_map(|b| {
                        b.content_json["inline"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                    })
                    .filter(|run| run["mention"] == to.to_string())
                    .collect();
                let wanted = runs.iter().find(|run| {
                    run["shown"].as_str() == link.shown.as_deref()
                        && run["block"].as_str() == link.block.map(|b| b.to_string()).as_deref()
                        && (run["embed"] == true) == (link.kind == "embed")
                });
                assert!(
                    wanted.is_some(),
                    "{what}: no such run among {runs:?}; blocks: {:#?}",
                    read.blocks(from)
                        .unwrap()
                        .iter()
                        .map(|b| (&b.kind, &b.content_json))
                        .collect::<Vec<_>>()
                );
            }
            "relation" => {
                let to = link.to_id.unwrap();
                let relation =
                    read.relations_from(from).unwrap().into_iter().find(|r| {
                        r.dst_id == to && r.field_id.is_none() && r.rel_type == "related"
                    });
                let relation = relation.unwrap_or_else(|| panic!("{what}: no relation"));
                assert_eq!(relation.label, link.label, "{what}");
            }
            "attachment" => {
                // A picture Obsidian named is not an app attachment (those are hash-named,
                // Decision 9; importing foreign files is M6): the embed stays text, the file
                // stays where it is, and Obsidian goes on showing it.
                let text: String = read
                    .blocks(from)
                    .unwrap()
                    .into_iter()
                    .flat_map(|b| {
                        b.content_json["inline"]
                            .as_array()
                            .cloned()
                            .unwrap_or_default()
                    })
                    .filter_map(|run| run["text"].as_str().map(str::to_owned))
                    .collect();
                assert!(
                    text.contains(&link.link),
                    "{what}: not kept as text: {text}"
                );
                assert!(read.attachments(from).unwrap().is_empty(), "{what}");
                assert!(
                    r.folder.join(&link.to).is_file(),
                    "{what}: the picture moved"
                );
                let bytes = fs::read(Path::new(FIXTURES).join("obsidian").join(&link.to)).unwrap();
                assert_eq!(fs::read(r.folder.join(&link.to)).unwrap(), bytes);
            }
            other => panic!("{what}: unknown kind {other}"),
        }
    }

    // Tags: `tagged` relations to Tag objects, by title, from the frontmatter and the body.
    let tag_titles = |id: Id| -> BTreeSet<String> {
        read.relations_from(id)
            .unwrap()
            .into_iter()
            .filter(|r| r.rel_type == "tagged")
            .map(|r| read.object(r.dst_id).unwrap().title)
            .collect()
    };
    let by_path: HashMap<&str, Id> = manifest
        .links
        .iter()
        .filter_map(|l| Some((l.to.as_str(), l.to_id?)))
        .collect();
    for (tag, files) in &manifest.tags {
        for file in files {
            let id = by_path[file.as_str()];
            assert!(
                tag_titles(id).contains(tag),
                "{file} should be tagged {tag}, has {:?}",
                tag_titles(id)
            );
        }
    }

    // The cloud drives' copies: the inbox, not the vault.
    let inbox = r.vault.conflicts(true).unwrap();
    for name in &manifest.cloud_conflict_files {
        assert!(
            inbox
                .iter()
                .any(|c| c.kind == ConflictKind::CloudConflictFile
                    && c.rel_path.as_deref() == Some(name.as_str())),
            "{name} should be in the inbox: {inbox:?}"
        );
    }
    assert_eq!(
        read.objects_titled(None, "Alpha").unwrap().len(),
        0,
        "the copies made no top-level Alpha"
    );
    assert_eq!(
        inbox
            .iter()
            .filter(|c| c.kind != ConflictKind::CloudConflictFile)
            .count(),
        0,
        "nothing else in the inbox: {inbox:?}"
    );
    r.vault.verify(true).unwrap();
}

#[test]
fn written_back_it_has_no_dangling_links_and_matches_the_golden() {
    let mut r = read_fixture();
    r.mirror.request_full_rewrite(&mut r.vault).unwrap();
    let out = r.mirror.pump_out(&mut r.vault).unwrap();
    assert!(out.pending.is_empty(), "{out:?}");

    // Zero dangling links, by the folder's own resolver.
    let resolver = VaultResolver::new(&r.vault.mirror_states(None).unwrap());
    let written = files_under(&r.folder);
    for (path, bytes) in &written {
        if !path.ends_with(".md") || path.starts_with(".dbme/") {
            continue;
        }
        let text = String::from_utf8(bytes.clone()).unwrap();
        for target in wikilinks(&text) {
            let resolved = if let Some(rel) = target.strip_prefix("attachments/") {
                r.folder.join("attachments").join(rel).is_file()
            } else {
                resolver.resolve(&target).is_some()
            };
            assert!(resolved, "{path}: [[{target}]] does not resolve");
        }
    }

    // The golden, ids normalised in order of first appearance across the sorted file list.
    let mut seen = HashMap::new();
    let mut actual: BTreeMap<String, String> = BTreeMap::new();
    for (path, bytes) in &written {
        if path.starts_with(".obsidian/") {
            continue;
        }
        let text = match String::from_utf8(bytes.clone()) {
            Ok(text) if !path.starts_with("attachments/") => text,
            _ => format!("<{} bytes>\n", bytes.len()),
        };
        actual.insert(normalise(path, &mut seen), normalise(&text, &mut seen));
    }
    let golden_dir = Path::new(FIXTURES).join("obsidian-roundtrip");
    if std::env::var_os("DBME_WRITE_FIXTURES").is_some() {
        if golden_dir.exists() {
            for (path, _) in files_under(&golden_dir) {
                fs::remove_file(golden_dir.join(path)).unwrap();
            }
        }
        for (path, text) in &actual {
            let target = golden_dir.join(path);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(target, text).unwrap();
        }
    }
    let golden: BTreeMap<String, String> = files_under(&golden_dir)
        .into_iter()
        .map(|(path, bytes)| {
            (
                path,
                String::from_utf8(bytes).unwrap().replace("\r\n", "\n"),
            )
        })
        .collect();
    assert_eq!(
        golden.keys().collect::<Vec<_>>(),
        actual.keys().collect::<Vec<_>>(),
        "the set of files differs from fixtures/obsidian-roundtrip (run with DBME_WRITE_FIXTURES=1 to rewrite it)"
    );
    for (path, text) in &actual {
        assert_eq!(
            &golden[path], text,
            "{path} differs from the golden (run with DBME_WRITE_FIXTURES=1 to rewrite it)"
        );
    }
    // A second write changes nothing: the fixpoint.
    r.mirror.request_full_rewrite(&mut r.vault).unwrap();
    r.mirror.pump_out(&mut r.vault).unwrap();
    assert_eq!(files_under(&r.folder), written);
}
