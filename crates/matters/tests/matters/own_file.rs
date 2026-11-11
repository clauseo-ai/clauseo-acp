//! A file the agent made with no tool call naming it (Bash running pandoc, say): proved absent
//! at the turn's start by its first checkpoint's skip record and last modified by the turn's
//! end, it is the agent's own, not flagged as maybe holding the lawyer's edits, and Undo deletes
//! it with no warning. Any doubt (a pre-existing file, a later mtime, a folder that did not
//! list) keeps the flag.

use std::time::{Duration, SystemTime};

use clauseo_matters::Matter;
use clauseo_protocol::FileChange;
#[cfg(unix)]
use clauseo_protocol::{SkippedFile, SkippedFileReason};

use crate::fixture::{
    Fixture, added, assert_none, at, cid, created, meta, modified, path, tools, undo,
};
use crate::support;

/// `secs` seconds after turn 3's end (before it when negative), as a file time.
fn around_end(secs: i64) -> SystemTime {
    let millis = at(3).as_millis() + secs * 1000;
    SystemTime::UNIX_EPOCH + Duration::from_millis(u64::try_from(millis).unwrap())
}

/// A matter holding `a.txt`, its turn 2 started: the folder is listed whole.
fn started(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    (fx, matter)
}

/// The agent writes `reply.docx` with no tool call naming it, last modified `secs` from the
/// turn's end; the turn ends with checkpoint 3.
fn made_reply(fx: &Fixture, matter: &mut Matter, secs: i64) -> Vec<FileChange> {
    fx.write("reply.docx", "the agent's reply");
    support::set_mtime(fx.file("reply.docx"), around_end(secs));
    created(matter.after_turn(&meta(3), &tools(&[])).unwrap(), 3)
}

#[test]
fn a_file_the_agent_made_unnamed_is_its_own_and_undo_deletes_it() {
    let (fx, mut matter) = started("own-unnamed");
    let changes = made_reply(&fx, &mut matter, -1);
    assert_eq!(changes, [added("reply.docx", b"the agent's reply")]);
    assert!(!changes[0].may_include_lawyer_edits());
    // The preview has nothing to ask about it.
    assert_none(&matter.edited_since(cid(1), cid(3)).unwrap());

    let undone = undo(&mut matter, (1, 3), &["reply.docx"], (4, 5)).unwrap();
    assert_eq!(undone.deleted, [path("reply.docx")]);
    assert_none(&undone.edited_since);
    assert_none(&undone.unversioned);
    assert!(!fx.exists("reply.docx"));
    assert_eq!(fx.read("a.txt"), b"one");
}

/// Its mtime at the turn's end exactly still counts as the turn's.
#[test]
fn a_file_modified_at_the_turns_end_is_the_agents() {
    let (fx, mut matter) = started("own-at-end");
    let changes = made_reply(&fx, &mut matter, 0);
    assert!(!changes[0].may_include_lawyer_edits());
}

/// Modified after the turn's end, when the checkpoint walked: maybe the lawyer's.
#[test]
fn a_file_modified_after_the_turns_end_is_flagged() {
    let (fx, mut matter) = started("own-after-end");
    let changes = made_reply(&fx, &mut matter, 5);
    assert_eq!(
        changes,
        [added("reply.docx", b"the agent's reply").with_lawyer_edits()]
    );
}

/// The agent's own file the lawyer then edits: the preview lists it, as any file changed since.
#[test]
fn the_agents_own_file_edited_after_the_turn_is_edited_since() {
    let (fx, mut matter) = started("own-edited-later");
    made_reply(&fx, &mut matter, -1);
    fx.write("reply.docx", "the lawyer's reply");
    support::set_mtime(fx.file("reply.docx"), around_end(60));
    assert_eq!(
        matter.edited_since(cid(1), cid(3)).unwrap(),
        [path("reply.docx")]
    );
}

/// A file that was there at the start, changed with no tool call naming it, stays flagged
/// whatever its mtime.
#[test]
fn a_file_there_at_the_start_changed_unnamed_is_flagged() {
    let (fx, mut matter) = started("own-pre-existing");
    fx.write("a.txt", "changed, unnamed");
    support::set_mtime(fx.file("a.txt"), around_end(-1));
    let changes = created(matter.after_turn(&meta(3), &tools(&[])).unwrap(), 3);
    assert_eq!(
        changes,
        [modified("a.txt", b"one", b"changed, unnamed").with_lawyer_edits()]
    );
}

/// A folder that did not list at the start: the new file may have come out of it, so it is
/// flagged, and Undo keeps it.
#[cfg(unix)]
#[test]
fn a_file_made_while_a_folder_was_unread_at_the_start_is_flagged_and_kept() {
    let fx = Fixture::new("own-unread-folder");
    fx.write("a.txt", "one");
    fx.write("Sub/lawyer.txt", "the lawyer's file");
    support::set_mode(fx.file("Sub"), 0o000);
    if std::fs::read_dir(fx.file("Sub")).is_ok() {
        support::set_mode(fx.file("Sub"), 0o755);
        eprintln!("skipped: this user reads a mode-000 folder");
        return;
    }
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::set_mode(fx.file("Sub"), 0o755);
    let changes = made_reply(&fx, &mut matter, -1);
    assert!(changes.iter().all(FileChange::may_include_lawyer_edits));
    assert!(changes.contains(&added("reply.docx", b"the agent's reply").with_lawyer_edits()));
    let both = ["Sub/lawyer.txt", "reply.docx"];
    let undone = undo(&mut matter, (1, 3), &both, (4, 5)).unwrap();
    assert_none(&undone.deleted);
    let kept: Vec<SkippedFile> = both
        .iter()
        .map(|rel| SkippedFile {
            path: path(rel),
            reason: SkippedFileReason::UnreadFolder,
        })
        .collect();
    assert_eq!(undone.unversioned, kept);
    assert_eq!(fx.read("reply.docx"), b"the agent's reply");
}
