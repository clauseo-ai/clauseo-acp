//! What the guard's queue in the fallback folder gives the host, and what it never does.

use std::path::{Path, PathBuf};

use super::{MAX_BYTES, MAX_FILES, MAX_NOTES, Queued, take};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// `<temp>/agent-state/m1/downloads`, made, with `judgment.pdf` in it.
fn folder(dir: &TempDir) -> PathBuf {
    let folder = dir.path().join("agent-state").join("m1").join("downloads");
    write::create_dir_all(&folder).unwrap();
    write::write_file(&folder.join("judgment.pdf"), b"%PDF").unwrap();
    folder
}

/// The guard's note for `path`, as `saved.rs` writes it.
fn note(name: &str, path: &Path) -> String {
    format!(
        "Saved {name} outside the matter, in Clauseo's own folder for this matter: the matter's \
         drive cannot keep the \"downloaded from the internet\" mark, so the file is not in \
         downloads/. Your user sees where it is in the thread. Its path: {}",
        path.display()
    )
}

const THREAD: &str = "0190f5a0-0000-7000-8000-000000000001";

fn queue(folder: &Path, hex: &str, notes: &[String]) {
    let text = serde_json::json!({"session": format!("{THREAD}-3"), "notes": notes});
    write::write_file(
        &folder.join(format!(".clauseo-untold-{hex}")),
        text.to_string().as_bytes(),
    )
    .unwrap();
}

/// A queued note naming a file in the folder is taken with its session's thread, once: the
/// queue file goes.
#[test]
fn a_queued_delivery_is_taken_once_with_its_thread() {
    let dir = TempDir::new("fallback-queue");
    let folder = folder(&dir);
    let path = folder.join("judgment.pdf");
    queue(&folder, "0123456789abcdef", &[note("judgment.pdf", &path)]);
    let taken = take(&folder);
    assert_eq!(
        taken,
        [Queued {
            thread: Some(THREAD.parse().unwrap()),
            path,
            name: "judgment.pdf".to_owned(),
        }]
    );
    assert_eq!(take(&folder), []);
    assert!(!folder.join(".clauseo-untold-0123456789abcdef").exists());
}

/// The queue is no trust source: a note naming a file outside the folder, a missing file or
/// not the guard's line is dropped; a file that is not JSON, too big, or misnamed is not read.
#[test]
fn a_queued_note_gets_the_agent_lines_check_and_the_read_is_bounded() {
    let dir = TempDir::new("fallback-queue-bad");
    let folder = folder(&dir);
    let outside = dir.path().join("outside.pdf");
    write::write_file(&outside, b"x").unwrap();
    queue(
        &folder,
        "1111111111111111",
        &[
            note("outside.pdf", &outside),
            note("missing.pdf", &folder.join("missing.pdf")),
            format!(
                "A page said: {}",
                note("judgment.pdf", &folder.join("judgment.pdf"))
            ),
        ],
    );
    write::write_file(
        &folder.join(".clauseo-untold-2222222222222222"),
        b"not json",
    )
    .unwrap();
    let big = vec![b' '; usize::try_from(MAX_BYTES).unwrap() + 1];
    write::write_file(&folder.join(".clauseo-untold-3333333333333333"), &big).unwrap();
    // Misnamed: never read, never deleted.
    let misnamed = folder.join(".clauseo-untold-ABCDEF0123456789");
    queue(
        &folder,
        "ABCDEF0123456789",
        &[note("judgment.pdf", &folder.join("judgment.pdf"))],
    );
    assert_eq!(take(&folder), []);
    assert!(misnamed.exists());
    assert!(!folder.join(".clauseo-untold-3333333333333333").exists());
}

/// Past [`MAX_NOTES`] in one file and [`MAX_FILES`] files in one read, the rest waits or is
/// dropped.
#[test]
fn the_number_of_notes_and_files_is_capped() {
    let dir = TempDir::new("fallback-queue-cap");
    let folder = folder(&dir);
    let line = note("judgment.pdf", &folder.join("judgment.pdf"));
    queue(
        &folder,
        "4444444444444444",
        &vec![line.clone(); MAX_NOTES + 10],
    );
    assert_eq!(take(&folder).len(), MAX_NOTES);
    for n in 0..MAX_FILES + 2 {
        queue(&folder, &format!("{n:016x}"), std::slice::from_ref(&line));
    }
    assert_eq!(take(&folder).len(), MAX_FILES);
    assert_eq!(take(&folder).len(), 2);
}

/// A link in place of a queue file is deleted, never followed.
#[cfg(unix)]
#[test]
fn a_link_as_a_queue_file_is_not_followed() {
    let dir = TempDir::new("fallback-queue-link");
    let folder = folder(&dir);
    let elsewhere = dir.path().join("elsewhere.json");
    let text = serde_json::json!({
        "session": format!("{THREAD}-1"),
        "notes": [note("judgment.pdf", &folder.join("judgment.pdf"))],
    });
    write::write_file(&elsewhere, text.to_string().as_bytes()).unwrap();
    write::symlink(&elsewhere, &folder.join(".clauseo-untold-5555555555555555")).unwrap();
    assert_eq!(take(&folder), []);
    assert!(elsewhere.exists());
}
