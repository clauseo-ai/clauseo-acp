//! The watch on NTFS: `ReadDirectoryChangesExW` armed on a matter, a lawyer's folder renamed to
//! `.git`, and an empty folder turned into a junction leading out (`mklink /J`'s two steps),
//! each set aside within the 2 s the decision record promises, the session ended first; and the
//! pairing's records: a folder moved out of a watched temp folder leaves it with the 64-bit file
//! ID the matter's `FILE_ACTION_ADDED` carries, and a file moved out isn't listed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::os::{DirWatch, Notify, Stop};
use super::{Changes, Guard, Next, Pulse, Session, Source, watch};
use crate::windows::denial_pipe::Record;
use crate::windows::set_aside::tests::launch_matter_on_test_volume;
use crate::windows::set_aside::{Failed, GitGuard, Hooks, Live};
use crate::windows::test_junction::set_mount_point;
use crate::windows::test_world::{World, matter_one};
use crate::windows::write::rename_new;

/// A fixed time for the pass folders.
fn at() -> SystemTime {
    UNIX_EPOCH + Duration::from_mins(29_372_295)
}

struct TestGuard(GitGuard);

impl Guard for TestGuard {
    fn changed(&self, changes: &Changes, hooks: &impl Hooks) -> Live {
        let mut live = self.0.changed(
            &changes.added,
            (&changes.moved_folders, &changes.fresh_paths()),
            at(),
            hooks,
        );
        live.moved
            .extend(self.0.relinked(&changes.relinked, at(), hooks).moved);
        live
    }

    fn rescan(&self, hooks: &impl Hooks) -> Live {
        self.0.rescan(at(), hooks)
    }

    fn retry(&self, failed: &[Failed], hooks: &impl Hooks) -> Live {
        self.0.retry(failed, at(), hooks)
    }
}

struct TestSession<'a> {
    stop: &'a Stop,
    said: Arc<Mutex<Vec<String>>>,
    pulse: Pulse,
}

impl Session for TestSession<'_> {
    fn end(&self) -> bool {
        self.said.lock().unwrap().push("end".to_owned());
        true
    }

    fn say(&self, line: &str) {
        self.said.lock().unwrap().push(line.to_owned());
    }

    fn record(&self, record: &Record) {
        if let Record::Notice { text } = record {
            self.said.lock().unwrap().push(text.clone());
        }
    }

    fn pause(&self, wait: Duration) -> bool {
        self.stop.wait(wait)
    }

    fn settle(&self, _wait: Duration) {}

    fn now(&self) -> Duration {
        Duration::ZERO
    }

    fn pulse(&self) -> &Pulse {
        &self.pulse
    }
}

/// Runs a watch on `root` while `act` changes it; whether `name` was gone from `root` within
/// 2 s, and what the session was told.
fn watched_while(root: &Path, data: &Path, act: impl FnOnce(), name: &str) -> (bool, Vec<String>) {
    let stop = Arc::new(Stop::new().unwrap());
    let said = Arc::new(Mutex::new(Vec::new()));
    let (thread_root, thread_data) = (root.to_owned(), data.to_owned());
    let (thread_stop, thread_said) = (Arc::clone(&stop), Arc::clone(&said));
    let (armed, first_armed) = std::sync::mpsc::channel();
    let watcher = thread::spawn(move || {
        let guard = TestGuard(GitGuard::new(&thread_root, &thread_data, &matter_one()));
        let session = TestSession {
            stop: &thread_stop,
            said: thread_said,
            pulse: Pulse::default(),
        };
        let first = DirWatch::arm(&thread_root, &thread_stop, Notify::All).expect("the watch arms");
        armed.send(()).unwrap();
        watch(first, &guard, &session);
    });
    first_armed.recv().unwrap();
    act();
    let tick = Stop::new().unwrap();
    let gone = (0..100).any(|_| !tick.wait(Duration::from_millis(20)) && !there(root, name));
    stop.set();
    watcher.join().unwrap();
    let said = said.lock().unwrap().clone();
    (gone, said)
}

#[test]
fn a_folder_renamed_to_git_under_a_watched_matter_is_set_aside_within_two_seconds() {
    let w = World::new("ntfs-git-watch");
    w.dir
        .file("matter/Precedents/hooks/post-checkout", "exit 0\r\n");
    w.dir.dir("data");
    let root = w.canonical("matter");
    let data = w.canonical("data");
    launch_matter_on_test_volume(&root, &data, &matter_one(), "s1", at()).unwrap();

    let rename = || rename_new(&root.join("Precedents"), &root.join(".git")).unwrap();
    let (gone, said) = watched_while(&root, &data, rename, ".git");

    assert!(gone, "set aside within 2 s");
    let end = said.iter().position(|line| line == "end").unwrap();
    // The lawyer's own folder, renamed by the agent: the lawyer's owner, read from the handle
    // the move renamed, so the notice says how to put it back.
    let notice = said
        .iter()
        .position(|line| line.contains("Clauseo moved .git to "))
        .unwrap();
    assert!(said[notice].contains("If you made it yourself, move it back"));
    assert!(end < notice, "the session ended first: {said:?}");
}

/// H1: the agent makes an empty folder, then sets a junction on it toward a folder outside the
/// matter holding a `.git` (`FSCTL_SET_REPARSE_POINT`, as `mklink /J` does, no privilege
/// needed). No name is made by the second step; the attribute change is reported, and the link
/// is set aside.
#[test]
fn an_empty_folder_turned_into_a_junction_leading_out_is_set_aside_within_two_seconds() {
    let w = World::new("ntfs-git-watch-junction");
    w.dir.file("temp/repo/.git/HEAD", "ref: refs/heads/main\n");
    w.dir.dir("data");
    let root = w.canonical("matter");
    let data = w.canonical("data");
    let target = w.canonical("temp/repo");
    w.dir.dir("matter/later");
    launch_matter_on_test_volume(&root, &data, &matter_one(), "s1", at()).unwrap();

    let relink = || set_mount_point(&root.join("later"), &target).unwrap();
    let (gone, said) = watched_while(&root, &data, relink, "later");

    assert!(gone, "set aside within 2 s: {said:?}");
    assert!(said.iter().any(|line| line == "end"));
    assert!(
        target.join(".git/HEAD").is_file(),
        "what it named is left alone"
    );
}

/// Whether `name` is in `root`, itself.
fn there(root: &Path, name: &str) -> bool {
    std::fs::symlink_metadata(root.join(name)).is_ok()
}

/// The pairing's records (`pairs.rs`): a folder moved from the agent's temp folder into the
/// matter leaves temp (`FILE_ACTION_REMOVED`, a watch for names only) with the same file ID as
/// the matter's `FILE_ACTION_ADDED`, which lists it fresh by its creation time; a file deleted
/// out of temp isn't listed. The ID's top 16 bits are NTFS's sequence number, never 0 for a file.
#[test]
fn a_folder_moved_out_of_temp_leaves_with_the_file_id_its_added_record_carries() {
    let w = World::new("ntfs-git-watch-pairing");
    w.dir.file("temp/x/inside.txt", "x");
    w.dir.file("temp/note.txt", "x");
    w.dir.dir("matter");
    w.dir.dir("elsewhere");
    let (temp, matter) = (w.canonical("temp"), w.canonical("matter"));
    let stop = Stop::new().unwrap();
    let mut from = DirWatch::arm(&temp, &stop, Notify::Names).unwrap();
    let mut into = DirWatch::arm(&matter, &stop, Notify::All).unwrap();
    rename_new(
        &temp.join("note.txt"),
        &w.canonical("elsewhere").join("note.txt"),
    )
    .unwrap();
    rename_new(&temp.join("x"), &matter.join("x")).unwrap();
    let left = gathered(&mut from).left;
    let fresh = gathered(&mut into).fresh_folders;
    assert_eq!(left.len(), 1, "the folder only: {left:?}");
    let (path, id) = &left[0];
    assert_eq!(path, Path::new("x"));
    assert_ne!(
        id >> 48,
        0,
        "the sequence number is in the top bits: {id:#x}"
    );
    assert_eq!(fresh, [(PathBuf::from("x"), *id)]);
}

/// What `watch` reads until it goes idle once something came, merged: at most five reads.
fn gathered(watch: &mut DirWatch<'_>) -> Changes {
    let mut all = Changes::default();
    let mut came = false;
    for _ in 0..5 {
        match watch.next() {
            Next::Changed(changes) => {
                came = true;
                all.left.extend(changes.left);
                all.fresh_folders.extend(changes.fresh_folders);
            }
            Next::Idle if came => break,
            Next::Idle => {}
            next @ (Next::Overflow | Next::Broken(_) | Next::Stopped) => panic!("{next:?}"),
        }
    }
    all
}
