use std::collections::BTreeSet;

use clauseo_protocol::MatterPath;

use super::{Candidate, Draft, chosen, draft};
use crate::Error;
use crate::checkpoint::VersionEntry;
use crate::history::Oid;
use crate::stat::{CacheEntry, EntryState, Stat};
use crate::types::RestoreFiles;

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

fn then(path_text: &str, blob: u8) -> VersionEntry {
    VersionEntry {
        path: path(path_text),
        blob: Oid([blob; 20]),
    }
}

fn now(path_text: &str, state: EntryState) -> CacheEntry {
    CacheEntry {
        path: path(path_text),
        stat: Stat::default(),
        state,
    }
}

fn read(blob: u8) -> EntryState {
    EntryState::Read(Oid([blob; 20]))
}

fn unread(blob: u8) -> EntryState {
    EntryState::Unread(Oid([blob; 20]))
}

fn candidate(path_text: &str, blob: u8, now: Option<u8>) -> Candidate {
    Candidate {
        path: path(path_text),
        blob: Oid([blob; 20]),
        now: now.map(|byte| Oid([byte; 20])),
    }
}

fn paths(texts: &[&str]) -> Vec<MatterPath> {
    texts.iter().map(|text| path(text)).collect()
}

/// The version and the folder of every case below, by path.
fn lists() -> (Vec<VersionEntry>, Vec<CacheEntry>) {
    let then = vec![
        then("brought-back.txt", 1),
        then("cloud-other.txt", 2),
        then("cloud-same.txt", 3),
        then("modified.txt", 4),
        then("over-cap-now.txt", 5),
        then("same.txt", 6),
    ];
    let now = vec![
        now("added-since.txt", read(7)),
        now("cloud-other.txt", unread(8)),
        now("cloud-same.txt", unread(3)),
        now("modified.txt", read(9)),
        now("never-versioned.bin", EntryState::Skipped),
        now("over-cap-now.txt", EntryState::Skipped),
        now("same.txt", read(6)),
    ];
    (then, now)
}

#[test]
fn whole_matter_writes_what_differs_keeps_skipped_and_leaves_files_added_since() {
    let (then, now) = lists();
    assert_eq!(
        draft(&then, &now, None).unwrap(),
        Draft {
            candidates: vec![
                candidate("brought-back.txt", 1, None),
                candidate("modified.txt", 4, Some(9)),
            ],
            added_since: paths(&["added-since.txt"]),
            kept: paths(&["cloud-other.txt", "over-cap-now.txt"]),
        }
    );
}

#[test]
fn chosen_files_restrict_every_list() {
    let (then, now) = lists();
    let chosen: BTreeSet<&str> = [
        "added-since.txt",
        "modified.txt",
        "never-versioned.bin",
        "same.txt",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        draft(&then, &now, Some(&chosen)).unwrap(),
        Draft {
            candidates: vec![candidate("modified.txt", 4, Some(9))],
            added_since: paths(&["added-since.txt"]),
            kept: paths(&["never-versioned.bin"]),
        }
    );
}

#[test]
fn a_chosen_file_in_neither_list_is_unknown() {
    let (then, now) = lists();
    let chosen: BTreeSet<&str> = ["modified.txt", "nowhere.txt"].into_iter().collect();
    let err = draft(&then, &now, Some(&chosen)).unwrap_err();
    assert!(
        matches!(&err, Error::UnknownFile { path: p } if *p == path("nowhere.txt")),
        "{err:?}"
    );
}

#[test]
fn a_folder_is_not_a_file() {
    let then = vec![then("drafts/a.txt", 1)];
    let now = vec![now("drafts/a.txt", read(1))];
    let chosen: BTreeSet<&str> = ["drafts"].into_iter().collect();
    assert!(matches!(
        draft(&then, &now, Some(&chosen)),
        Err(Error::UnknownFile { .. })
    ));
}

#[test]
fn chosen_list_must_name_files_once() {
    assert!(matches!(
        chosen(RestoreFiles::Only(&[])),
        Err(Error::NoFilesChosen)
    ));
    let twice = paths(&["a.txt", "a.txt"]);
    assert!(matches!(
        chosen(RestoreFiles::Only(&twice)),
        Err(Error::UnknownFile { path: p }) if p == path("a.txt")
    ));
    assert_eq!(chosen(RestoreFiles::All).unwrap(), None);
}

#[test]
fn equal_lists_draft_nothing() {
    let then = vec![then("a.txt", 1)];
    let now = vec![now("a.txt", read(1))];
    assert_eq!(draft(&then, &now, None).unwrap(), Draft::default());
}
