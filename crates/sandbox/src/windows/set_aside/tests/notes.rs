//! The notes folder's own passes (review item 3): its record beside the matter's, the launch
//! pass that checks what a crashed session left there, the after-session pass that moves what
//! the live watch missed, and the notes watch that knows only what that record holds.

use std::path::{Path, PathBuf};

use super::live::{calm, origins, reported};
use super::{exit, launch, list, record, volume, world};
use crate::windows::set_aside::bookkeeping::{SetAsideBookkeeping, forget_bookkeeping};
use crate::windows::set_aside::files::Place;
use crate::windows::set_aside::live::Watched;
use crate::windows::set_aside::{Checked, Reason, run};
use crate::windows::test_world::matter_one;

const HEAD: &str = "ref: refs/heads/main\n";

/// The notes folder's launch pass for `session`, with `running` still running.
fn notes_launch(notes: &Path, data: &Path, session: &str, running: &[&str]) -> Checked {
    run::notes_before_launch(
        &volume(),
        notes,
        data,
        &matter_one(),
        session,
        running,
        launch(),
    )
    .unwrap()
}

/// The notes folder's after-session pass for `session`.
fn notes_exit(notes: &Path, data: &Path, session: &str) -> Checked {
    run::notes_after_session(&volume(), notes, data, &matter_one(), session, exit()).unwrap()
}

/// Where each item moved came from.
fn from(checked: &Checked) -> Vec<PathBuf> {
    checked
        .moved
        .iter()
        .map(|entry| entry.from.clone())
        .collect()
}

/// The notes record as saved.
fn notes_record(data: &Path) -> crate::windows::set_aside::files::Record {
    Place::notes(data, &matter_one())
        .read_record()
        .unwrap()
        .unwrap()
}

/// A lawyer's repository in notes at the first launch is recorded in the notes record, not the
/// matter's, and the notes watch knows it; a `.git` a crashed session left in notes is set aside
/// by the next launch's pass, into the matter's set-aside.
#[test]
fn a_git_in_notes_at_launch_is_handled_by_the_launch_pass() {
    let (w, root, data) = world("notes-launch");
    w.dir.file("notes/theirs/.git/HEAD", HEAD);
    let notes = w.canonical("notes");
    run::before_launch(&volume(), &root, &data, &matter_one(), "s1", &[], launch()).unwrap();
    let first = notes_launch(&notes, &data, "s1", &[]);
    assert!(first.moved.is_empty() && first.unscanned.is_empty());
    let key = vec!["theirs".into(), ".git".into()];
    assert!(notes_record(&data).names.contains_key(&key));
    assert!(!record(&data).names.contains_key(&key), "kept apart");
    let watch = Watched::new_notes(&volume(), &notes, &data, &matter_one());
    let live = watch.changed(&volume(), &reported(&["theirs/.git"]), exit(), &calm());
    assert!(live.moved.is_empty(), "the record holds it");

    // s1's agent makes a repository in notes and s1 crashes: s2's launch checks it.
    w.dir.file("notes/planted/.git/HEAD", HEAD);
    let second = notes_launch(&notes, &data, "s2", &[]);
    assert_eq!(from(&second), [notes.join("planted/.git")]);
    assert_eq!(second.moved[0].reason, Reason::New);
    assert_eq!(second.moved[0].matter, matter_one());
    assert!(
        notes.join("theirs/.git/HEAD").is_file(),
        "the lawyer's stays"
    );
    assert_eq!(list(&data).len(), 1, "listed in the matter's manifest");
    let open = notes_record(&data).sessions;
    assert_eq!(
        open,
        ["s2".to_owned()].into(),
        "the crashed session was dropped"
    );
}

/// A `.git` made in notes that the live watch missed is moved by the after-session pass; the
/// next session's notes watch then holds nothing at that path, so one made there again goes.
#[test]
fn a_git_the_watch_missed_in_notes_is_moved_after_the_session() {
    let (w, _root, data) = world("notes-exit");
    w.dir.file("notes/today.md", "");
    let notes = w.canonical("notes");
    notes_launch(&notes, &data, "s1", &[]);

    w.dir.file("notes/repo/.git/HEAD", HEAD);
    let after = notes_exit(&notes, &data, "s1");
    assert_eq!(from(&after), [notes.join("repo/.git")]);
    assert!(!notes.join("repo/.git").exists());
    assert!(notes.join("today.md").is_file());
    assert!(notes_record(&data).sessions.is_empty(), "closed");

    notes_launch(&notes, &data, "s2", &[]);
    let watch = Watched::new_notes(&volume(), &notes, &data, &matter_one());
    w.dir.file("notes/repo/.git/HEAD", HEAD);
    let live = watch.changed(&volume(), &reported(&["repo/.git"]), exit(), &calm());
    assert_eq!(
        origins(&live),
        [notes.join("repo/.git")],
        "not counted as known"
    );
}

/// The notes watch starts from the launch pass's record, not from a scan of its own: a `.git`
/// made in notes after the launch pass and before the watch starts is not known, and goes.
#[test]
fn the_notes_watch_knows_only_what_the_launch_pass_recorded() {
    let (w, _root, data) = world("notes-watch-record");
    w.dir.dir("notes");
    let notes = w.canonical("notes");
    notes_launch(&notes, &data, "s1", &[]);

    w.dir.file("notes/early/.git/HEAD", HEAD);
    let watch = Watched::new_notes(&volume(), &notes, &data, &matter_one());
    let live = watch.changed(&volume(), &reported(&["early/.git"]), exit(), &calm());
    assert_eq!(origins(&live), [notes.join("early/.git")]);
}

/// A notes folder not there at launch is recorded as empty, not as a partial scan; one the
/// session makes, holding a `.git`, is checked after it.
#[test]
fn a_notes_folder_made_during_the_session_is_checked_after_it() {
    let (w, root, data) = world("notes-missing");
    let notes = root.parent().unwrap().join("notes");
    let first = notes_launch(&notes, &data, "s1", &[]);
    assert_eq!(first, Checked::default());

    w.dir.file("notes/new/.git/HEAD", HEAD);
    let after = notes_exit(&notes, &data, "s1");
    assert_eq!(from(&after), [notes.join("new/.git")]);
    assert_eq!(after.unscanned, Vec::<PathBuf>::new());
}

/// The notes record is bookkeeping: `--remove` keeps the matter's folder while it lists an open
/// session, and takes it whole once both records are closed.
#[test]
fn forget_keeps_an_open_notes_record_and_takes_a_closed_one() {
    let (w, root, data) = world("notes-forget");
    w.dir.dir("notes");
    let notes = w.canonical("notes");
    run::before_launch(&volume(), &root, &data, &matter_one(), "s1", &[], launch()).unwrap();
    notes_launch(&notes, &data, "s1", &[]);
    run::after_session(&volume(), &root, &data, &matter_one(), "s1", exit()).unwrap();
    let folder = data
        .join("sandbox/set-aside")
        .join(matter_one().to_string());
    assert!(folder.join("notes-record.json").is_file());

    let kept = forget_bookkeeping(&data, &matter_one());
    assert_eq!(
        kept,
        Ok(SetAsideBookkeeping::Left),
        "the notes session is open"
    );
    notes_exit(&notes, &data, "s1");
    let gone = forget_bookkeeping(&data, &matter_one());
    assert_eq!(gone, Ok(SetAsideBookkeeping::Gone));
    assert!(std::fs::symlink_metadata(&folder).is_err());
}
