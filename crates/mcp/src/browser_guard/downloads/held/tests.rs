//! Held downloads: Chrome's names for them, the note, the mover's record and the clearing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use super::super::{Mover, Sweeper, sweep};
use super::{Held, clear, is_held, note};
use crate::write;

/// A fresh `<temp>/clauseo-mcp-held-<test>-<pid>` with empty `staging` and `downloads`.
fn scratch(test: &str) -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-held-{test}-{}", std::process::id()));
    if dir.exists() {
        write::remove_dir_all(&dir).unwrap();
    }
    let (staging, downloads) = (dir.join("staging"), dir.join("downloads"));
    write::create_dir_all(&staging).unwrap();
    write::create_dir_all(&downloads).unwrap();
    (dir, staging, downloads)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A tool call's reply with one text block of agent-browser's.
fn reply() -> Value {
    json!({"jsonrpc": "2.0", "id": 7, "result": {"content": [{"type": "text", "text": "ok"}]}})
}

/// `note_into` on `reply`, parsed.
fn noted(held: &Held, reply: &Value) -> Option<Value> {
    let bytes = held.note_into(reply.to_string().as_bytes())?;
    Some(serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn chromes_held_names_in_any_language_are_held_and_nothing_else() {
    for name in [
        "Unconfirmed 0.crdownload",
        "Unconfirmed 123456.crdownload",
        "Unconfirmed 1000000.crdownload",
        "Nicht bestätigt 42.crdownload",
        "未確認 7.crdownload",
    ] {
        assert!(is_held(name), "{name}");
    }
    for name in [
        "report.pdf.crdownload",
        "Report 12.pdf.crdownload",
        "Unconfirmed 12345678.crdownload",
        "Unconfirmed .crdownload",
        "Unconfirmed 12a.crdownload",
        " 12.crdownload",
        "v1.2 34.crdownload",
        "Unconfirmed 123456",
        "Unconfirmed 123456.tmp",
        "setup.exe",
    ] {
        assert!(!is_held(name), "{name}");
    }
}

/// The note goes after the reply's own content, which stays as it was, once; a reply without
/// content gets it as its only block; an error reply waits for the next result.
#[test]
fn the_note_rides_the_next_result_once_after_its_own_content() {
    let held = Held::default();
    assert_eq!(noted(&held, &reply()), None, "nothing held, nothing told");
    held.record();
    let error = json!({"jsonrpc": "2.0", "id": 7, "error": {"code": -1, "message": "no"}});
    assert_eq!(noted(&held, &error), None);
    let told = noted(&held, &reply()).unwrap();
    let content = told["result"]["content"].as_array().unwrap();
    assert_eq!(content.len(), 2, "{told}");
    assert_eq!(content[0], reply()["result"]["content"][0]);
    assert_eq!(content[1], json!({"type": "text", "text": note(1)}));
    assert_eq!(told["id"], 7);
    assert_eq!(noted(&held, &reply()), None, "told twice");
    held.record();
    held.record();
    let bare = json!({"jsonrpc": "2.0", "id": 8, "result": {}});
    let told = noted(&held, &bare).unwrap();
    assert_eq!(
        told["result"]["content"],
        json!([{"type": "text", "text": note(2)}])
    );
    assert!(note(2).contains("2 downloads"), "{}", note(2));
    assert!(note(1).starts_with("Clauseo does not deliver executable downloads."));
}

/// The mover records each held file once, never moves it, and delivers what finished beside it.
#[test]
fn the_mover_records_a_held_download_once_and_never_moves_it() {
    let (dir, staging, downloads) = scratch("record");
    let held = Arc::new(Held::default());
    let mut sweeper = Sweeper::new(staging.clone(), downloads.clone(), 0);
    sweeper.held = Some(Arc::clone(&held));
    write::write(&staging.join("Unconfirmed 31337.crdownload"), "MZ").unwrap();
    write::write(&staging.join("notes.txt"), "n").unwrap();
    sweeper.sweep();
    sweeper.sweep();
    assert_eq!(names(&downloads), ["notes.txt"]);
    assert_eq!(names(&staging), ["Unconfirmed 31337.crdownload"]);
    let told = noted(&held, &reply()).unwrap();
    assert_eq!(told["result"]["content"][1]["text"], note(1));
    write::remove_dir_all(&dir).unwrap();
}

/// A held name gone by the next sweep (a page's own `Report 12`, finished at once) is never told;
/// one still there at its second sweep is, once.
#[test]
fn a_held_name_counts_only_once_it_stays_for_two_sweeps() {
    let (dir, staging, downloads) = scratch("two-sweeps");
    let held = Arc::new(Held::default());
    let mut sweeper = Sweeper::new(staging.clone(), downloads.clone(), 0);
    sweeper.held = Some(Arc::clone(&held));
    write::write(&staging.join("Report 12.crdownload"), "half").unwrap();
    sweeper.sweep();
    write::replace(
        &staging.join("Report 12.crdownload"),
        &staging.join("Report 12"),
    )
    .unwrap();
    sweeper.sweep();
    assert_eq!(names(&downloads), ["Report 12"]);
    assert_eq!(
        noted(&held, &reply()),
        None,
        "a finished download was told as held"
    );
    write::write(&staging.join("Unconfirmed 5.crdownload"), "MZ").unwrap();
    sweeper.sweep();
    assert_eq!(noted(&held, &reply()), None, "told at its first sweep");
    sweeper.sweep();
    assert!(
        noted(&held, &reply()).is_some(),
        "not told at its second sweep"
    );
    sweeper.sweep();
    assert_eq!(noted(&held, &reply()), None, "told twice");
    write::remove_dir_all(&dir).unwrap();
}

/// When Chrome stops (the mover's finish, the reaper's sweep), held files go; a download still
/// in progress under its own name stays, as before.
#[test]
fn a_stop_clears_held_downloads_and_keeps_the_rest() {
    let (dir, staging, downloads) = scratch("clear");
    let mover = Mover::start(
        staging.clone(),
        downloads.clone(),
        Arc::default(),
        Arc::default(),
    );
    write::write(&staging.join("Unconfirmed 1.crdownload"), "MZ").unwrap();
    write::write(&staging.join("big.zip.crdownload"), "half").unwrap();
    mover.finish();
    assert_eq!(names(&staging), ["big.zip.crdownload"]);
    write::write(&staging.join("Unconfirmed 2.crdownload"), "MZ").unwrap();
    sweep(&staging, &downloads);
    assert_eq!(names(&staging), ["big.zip.crdownload"]);
    write::write(&staging.join("Unconfirmed 3.crdownload"), "MZ").unwrap();
    clear(&staging);
    clear(&dir.join("missing"));
    assert_eq!(names(&staging), ["big.zip.crdownload"]);
    write::remove_dir_all(&dir).unwrap();
}
