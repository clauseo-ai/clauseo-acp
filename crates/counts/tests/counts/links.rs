//! A link where a counts folder goes (deletion audit of S20): reading the folder back never
//! follows it, so nothing outside the data folder is removed.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use clauseo_counts::Counts;

use crate::support::*;
use crate::write::link_dir;

/// A folder outside the data folder holding a file, a folder with a file in it, a dot-file, a
/// JSON file that is no queued day or tally and a `queue` folder: everything the counts load
/// would otherwise remove.
fn canary(scratch: &Scratch) -> PathBuf {
    let canary = scratch.path().join("canary");
    plant(&canary.join("brief.docx"), b"privileged");
    plant(&canary.join("exhibits/a.pdf"), b"exhibit");
    plant(&canary.join(".notes"), b"notes");
    plant(&canary.join("2026-11-01.json"), b"{}");
    // What a link at `counts` itself would expose to the queue's load.
    plant(&canary.join("queue/held.docx"), b"held");
    canary
}

fn assert_untouched(canary: &Path) {
    assert_eq!(
        files(canary),
        [
            ".notes",
            "2026-11-01.json",
            "brief.docx",
            "exhibits/a.pdf",
            "queue/held.docx"
        ]
    );
}

/// Opens counts with a link at `<data>/<place>` to the canary, and checks the open fails and
/// leaves the canary (and the link) as they were.
fn link_at(place: &str) {
    let scratch = Scratch::new(&format!("link-{}", place.replace('/', "-")));
    let canary = canary(&scratch);
    let data = scratch.path().join("data");
    let link = data.join(place);
    plant(&data.join("first_act_done"), b"");
    link_dir(&link, &canary);
    let counts = Counts::open(&data, about(), true, true, SEED);
    let result = counts.tick(at(4, 60));
    drop(counts);
    assert_untouched(&canary);
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{err}");
}

#[test]
fn a_link_at_the_queue_folder_empties_nothing_it_points_at() {
    link_at("counts/queue");
}

#[test]
fn a_link_at_the_tally_folder_empties_nothing_it_points_at() {
    link_at("counts/tally");
}

#[test]
fn a_link_at_the_counts_folder_empties_nothing_it_points_at() {
    link_at("counts");
}
