//! The set-aside's logic through its [`Volume`] seam, on every OS: the NTFS calls on Windows,
//! a stand-in over inode numbers elsewhere. Each change below is one the old owner-based check
//! let through or didn't record.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::files::Place;
use super::run::{self, Refused, Volume};
use super::time::{rfc3339, timestamp};
use super::{Checked, Entry, Reason, SetAsideFailure};
use crate::windows::test_world::{STATE, World, matter_one};
use crate::windows::write::rename_new;

mod crash;
mod fast;
mod links;
mod nested;
#[cfg(unix)]
mod posix;
#[cfg(unix)]
mod round4;
#[cfg(unix)]
mod scans;
mod sessions;

#[cfg(windows)]
fn volume() -> super::ntfs::Ntfs {
    super::ntfs::Ntfs
}

#[cfg(unix)]
fn volume() -> posix::Posix {
    posix::Posix
}

/// 2025-11-05 10:15:00.123 UTC.
fn launch() -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(1_762_337_700_123)
}

/// An hour after [`launch`].
fn exit() -> SystemTime {
    launch() + Duration::from_secs(3600)
}

/// A world, its matter's folder as the policy holds it, and its data folder.
fn world(test: &str) -> (World, PathBuf, PathBuf) {
    let w = World::new(test);
    let root = w.canonical("matter");
    let data = w.canonical("data");
    (w, root, data)
}

/// What moved, from a pass whose scan must have been complete.
fn complete(checked: Checked) -> Vec<Entry> {
    assert_eq!(checked.unscanned, Vec::<PathBuf>::new(), "a partial scan");
    checked.moved
}

/// Session `session`'s launch through `volume`, with the sessions in `running` still running.
fn launch_as(
    volume: &impl Volume,
    root: &Path,
    data: &Path,
    session: &str,
    running: &[&str],
    now: SystemTime,
) -> Result<Vec<Entry>, Refused> {
    run::before_launch(volume, root, data, &matter_one(), session, running, now).map(complete)
}

/// Session `session`'s after-session pass through `volume`.
fn exit_as(
    volume: &impl Volume,
    root: &Path,
    data: &Path,
    session: &str,
    now: SystemTime,
) -> Result<Vec<Entry>, Refused> {
    run::after_session(volume, root, data, &matter_one(), session, now).map(complete)
}

/// `before_launch` over the tests' volume (NTFS on Windows, the stand-in elsewhere), for the
/// apply tests' R1 case.
pub(in crate::windows) fn launch_on_test_volume(
    root: &Path,
    data: &Path,
    session: &str,
    now: SystemTime,
) -> Result<Checked, SetAsideFailure> {
    run::before_launch(&volume(), root, data, &matter_one(), session, &[], now)
        .map_err(|refused| refused.failure)
}

/// `after_session` over the tests' volume, as [`launch_on_test_volume`].
pub(in crate::windows) fn exit_on_test_volume(
    root: &Path,
    data: &Path,
    session: &str,
    now: SystemTime,
) -> Result<Checked, SetAsideFailure> {
    run::after_session(&volume(), root, data, &matter_one(), session, now)
        .map_err(|refused| refused.failure)
}

/// Session `s1`'s launch, no other session running.
fn before(root: &Path, data: &Path, now: SystemTime) -> Result<Vec<Entry>, SetAsideFailure> {
    launch_as(&volume(), root, data, "s1", &[], now).map_err(|refused| refused.failure)
}

/// Session `s1`'s after-session pass.
fn after(root: &Path, data: &Path, now: SystemTime) -> Result<Vec<Entry>, SetAsideFailure> {
    exit_as(&volume(), root, data, "s1", now).map_err(|refused| refused.failure)
}

/// The matter's record as saved.
fn record(data: &Path) -> super::files::Record {
    Place::new(data, &matter_one())
        .read_record()
        .unwrap()
        .unwrap()
}

/// Every item listed, settled.
fn list(data: &Path) -> Vec<Entry> {
    run::list(&volume(), data, &matter_one()).unwrap()
}

/// The record's key for `rel`, a path below the matter's root with its parts joined by `/`.
fn key(rel: &str) -> Vec<std::ffi::OsString> {
    rel.split('/').map(std::ffi::OsString::from).collect()
}

/// No entries, for the comparisons.
fn none() -> Vec<Entry> {
    Vec::new()
}

/// Where a pass at `now` puts `name` (a path below the pass's folder, parts joined by `/`).
fn aside(data: &Path, now: SystemTime, name: &str) -> PathBuf {
    name.split('/').fold(
        data.join("sandbox/set-aside")
            .join(matter_one().to_string())
            .join(timestamp(now)),
        |path, part| path.join(part),
    )
}

#[test]
fn a_lawyer_folder_renamed_to_git_after_launch_is_set_aside() {
    let (w, root, data) = world("aside-renamed-git");
    w.dir
        .file("matter/Precedents/hooks/post-checkout", "#!/bin/sh\n");
    assert_eq!(before(&root, &data, launch()).unwrap(), none());

    rename_new(&root.join("Precedents"), &root.join(".git")).unwrap();
    let moved = after(&root, &data, exit()).unwrap();

    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].from, root.join(".git"));
    assert_eq!(moved[0].to, aside(&data, exit(), ".git"));
    assert_eq!(moved[0].reason, Reason::New);
    assert!(!root.join(".git").exists());
    assert!(moved[0].to.join("hooks/post-checkout").is_file());
}

#[test]
fn a_hard_link_named_claude_md_is_set_aside_whoever_owns_it() {
    let (w, root, data) = world("aside-hard-link");
    let lawyer = w.dir.file("matter/brief.md", "# the lawyer's\n");
    before(&root, &data, launch()).unwrap();

    w.dir.hard_link("matter/CLAUDE.md", &lawyer);
    let moved = after(&root, &data, exit()).unwrap();

    let names: Vec<_> = moved
        .iter()
        .map(|e| (e.name.to_str().unwrap(), e.reason))
        .collect();
    assert_eq!(names, [("CLAUDE.md", Reason::New)]);
    assert!(!root.join("CLAUDE.md").exists());
    assert!(lawyer.is_file(), "the lawyer's own name stays");
}

#[test]
fn names_unchanged_since_launch_stay() {
    let (w, root, data) = world("aside-unchanged");
    w.dir.file("matter/CLAUDE.md", "# the lawyer's\n");
    w.dir.file("matter/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.file("matter/sub/.git/HEAD", "ref: refs/heads/main\n");
    launch_as(&volume(), &root, &data, "s1", &[], launch()).unwrap();
    w.dir
        .file("matter/.git/index", "written by the agent's git");
    w.dir
        .file("matter/sub/.git/index", "written by the agent's git");
    w.dir.file("matter/CLAUDE.md", "# edited in place\n");

    let moved = exit_as(&volume(), &root, &data, "s1", exit()).unwrap();
    assert_eq!(moved, none());
    assert!(root.join("CLAUDE.md").is_file());
    assert!(root.join(".git/HEAD").is_file());
    assert!(root.join("sub/.git/HEAD").is_file());
    assert_eq!(list(&data), none());
}

#[test]
fn a_name_present_at_launch_and_replaced_is_set_aside_as_changed() {
    let (w, root, data) = world("aside-replaced");
    w.dir.file("matter/AGENTS.md", "# the lawyer's\n");
    let agents = w.dir.file("matter/draft.md", "# the agent's\n");
    before(&root, &data, launch()).unwrap();

    rename_new(&root.join("AGENTS.md"), &root.join("old.md")).unwrap();
    rename_new(&agents, &root.join("AGENTS.md")).unwrap();
    let moved = after(&root, &data, exit()).unwrap();

    let names: Vec<_> = moved
        .iter()
        .map(|e| (e.name.to_str().unwrap(), e.reason))
        .collect();
    assert_eq!(names, [("AGENTS.md", Reason::Changed)]);
    assert!(root.join("old.md").is_file());
}

#[test]
fn a_session_with_no_after_pass_is_checked_at_the_next_launch_first() {
    let (w, root, data) = world("aside-unclean");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/.mcp.json", "{}");

    let moved = before(&root, &data, exit()).unwrap();
    let names: Vec<_> = moved.iter().map(|e| e.name.to_str().unwrap()).collect();
    assert_eq!(names, [".mcp.json"]);
    assert!(!root.join(".mcp.json").exists());

    // Recorded afresh, so a name the lawyer adds after a clean end is never moved.
    w.dir.file("matter/.mcp.json", "{\"the\": \"lawyer's\"}");
    let later = exit() + Duration::from_secs(60);
    assert_eq!(after(&root, &data, later).unwrap().len(), 1);
    w.dir
        .file("matter/CLAUDE.md", "# the lawyer's, between sessions\n");
    assert_eq!(before(&root, &data, later).unwrap(), none());
    assert!(root.join("CLAUDE.md").is_file());
}

#[test]
fn an_after_pass_with_no_record_refuses() {
    let (_w, root, data) = world("aside-no-record");
    assert_eq!(after(&root, &data, exit()), Err(SetAsideFailure::Record));
}

#[test]
fn a_record_that_cant_be_read_refuses_the_launch() {
    let (w, root, data) = world("aside-bad-record");
    before(&root, &data, launch()).unwrap();
    let record = format!("data/sandbox/set-aside/{}/record.json", matter_one());
    w.dir.file(&record, "{\"version\": 9}");
    w.dir.file("matter/.mcp.json", "{}");
    assert_eq!(before(&root, &data, exit()), Err(SetAsideFailure::Record));
    assert!(root.join(".mcp.json").exists());
}

#[test]
fn the_set_aside_lives_under_the_sandbox_folder_not_the_state_folder() {
    let (w, root, data) = world("aside-location");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/.claude/settings.json", "{}");
    let moved = after(&root, &data, exit()).unwrap();
    let folder = data
        .join("sandbox/set-aside")
        .join(matter_one().to_string());
    assert!(moved[0].to.starts_with(folder.join("20251105T111500-123Z")));
    assert!(moved[0].to.join("settings.json").is_file());
    assert!(!w.at(STATE).join("set-aside").exists());
    assert!(folder.join("record.json").is_file());
    assert!(folder.join("manifest.json").is_file());
}

#[test]
fn restore_refuses_a_taken_name_and_restores_to_a_free_one() {
    let (w, root, data) = world("aside-restore");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/CLAUDE.md", "# the agent's\n");
    let entry = after(&root, &data, exit()).unwrap().remove(0);

    w.dir.file("matter/CLAUDE.md", "# the lawyer's new one\n");
    assert_eq!(
        run::restore(&volume(), &data, &entry),
        Err(SetAsideFailure::NameTaken.into())
    );
    assert_eq!(
        std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
        "# the lawyer's new one\n",
        "never overwritten"
    );
    assert!(entry.to.is_file());
    assert_eq!(list(&data), std::slice::from_ref(&entry));

    rename_new(&root.join("CLAUDE.md"), &root.join("lawyer.md")).unwrap();
    run::restore(&volume(), &data, &entry).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
        "# the agent's\n"
    );
    assert_eq!(list(&data), none());
    assert_eq!(
        run::restore(&volume(), &data, &entry),
        Err(SetAsideFailure::Gone.into())
    );
}

#[test]
fn the_manifest_lists_each_entry_with_its_reason() {
    let (w, root, data) = world("aside-manifest");
    w.dir.file("matter/AGENTS.md", "# the lawyer's\n");
    let swapped = w.dir.file("matter/swap.md", "# the agent's\n");
    let locked = w.dir.file("matter/sub/locked.txt", "shut out");
    before(&root, &data, launch()).unwrap();
    w.dir.file("matter/.codex/config.toml", "model = \"x\"\n");
    rename_new(&root.join("AGENTS.md"), &root.join("kept.md")).unwrap();
    rename_new(&swapped, &root.join("AGENTS.md")).unwrap();
    after(&root, &data, exit()).unwrap();
    let later = exit() + Duration::from_secs(1);
    let walked = run::set_aside_path(
        &volume(),
        &data,
        &matter_one(),
        &w.canonical("matter/sub/locked.txt"),
        Reason::LawyerCantOpen,
        later,
    )
    .unwrap();

    let listed = list(&data);
    let seen: Vec<_> = listed
        .iter()
        .map(|e| (e.id, e.name.to_str().unwrap(), e.reason))
        .collect();
    assert_eq!(
        seen,
        [
            (1, ".codex", Reason::New),
            (2, "AGENTS.md", Reason::Changed),
            (3, "locked.txt", Reason::LawyerCantOpen),
        ]
    );
    assert_eq!(listed[2], walked);
    assert_eq!(walked.to, aside(&data, later, "locked.txt"));
    assert_eq!(walked.at, "2025-11-05T11:15:01.123Z");
    assert!(listed.iter().all(|e| e.matter == matter_one()));
    assert!(listed.iter().all(|e| e.owner.is_some()), "{listed:?}");
    assert!(!locked.exists());
}

#[test]
fn a_taken_name_in_the_pass_folder_gets_the_next_free_one() {
    let (w, _root, data) = world("aside-next-name");
    let one = w.dir.file("matter/a/notes.txt", "one");
    let two = w.dir.file("matter/b/notes.txt", "two");
    let set = |path: &Path| {
        let path = dunce::canonicalize(path).unwrap();
        run::set_aside_path(
            &volume(),
            &data,
            &matter_one(),
            &path,
            Reason::LawyerCantOpen,
            exit(),
        )
        .unwrap()
    };
    assert_eq!(set(&one).name, "notes.txt");
    assert_eq!(set(&two).name, "notes.txt~2");
}

#[test]
fn timestamps_are_utc() {
    assert_eq!(timestamp(UNIX_EPOCH), "19700101T000000-000Z");
    let leap_day_2000: u64 = 951_782_400;
    let leap = UNIX_EPOCH + Duration::from_secs(leap_day_2000);
    assert_eq!(timestamp(leap), "20000229T000000-000Z");
    assert_eq!(rfc3339(launch()), "2025-11-05T10:15:00.123Z");
}
