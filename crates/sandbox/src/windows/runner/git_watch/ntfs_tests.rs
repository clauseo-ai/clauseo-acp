//! The watch on NTFS: `ReadDirectoryChangesW` armed on a matter, a lawyer's folder renamed to
//! `.git`, and the item set aside within the 2 s the decision record promises.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::os::{DirWatch, Stop};
use super::{Guard, Session, watch};
use crate::windows::set_aside::tests::launch_matter_on_test_volume;
use crate::windows::set_aside::{Failed, GitGuard, Live};
use crate::windows::test_world::{World, matter_one};
use crate::windows::write::rename_new;

/// A fixed time for the pass folders.
fn at() -> SystemTime {
    UNIX_EPOCH + Duration::from_mins(29_372_295)
}

struct TestGuard(GitGuard);

impl Guard for TestGuard {
    fn changed(&self, paths: &[std::path::PathBuf]) -> Live {
        self.0.changed(paths, at())
    }

    fn rescan(&self) -> Live {
        self.0.rescan(at())
    }

    fn retry(&self, failed: &[Failed]) -> Live {
        self.0.retry(failed, at())
    }
}

struct TestSession<'a> {
    stop: &'a Stop,
    said: Arc<Mutex<Vec<String>>>,
}

impl Session for TestSession<'_> {
    fn end(&self) {
        self.said.lock().unwrap().push("end".to_owned());
    }

    fn say(&self, line: &str) {
        self.said.lock().unwrap().push(line.to_owned());
    }

    fn pause(&self, wait: Duration) -> bool {
        self.stop.wait(wait)
    }

    fn settle(&self, _wait: Duration) {}
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

    let stop = Arc::new(Stop::new().unwrap());
    let said = Arc::new(Mutex::new(Vec::new()));
    let (thread_root, thread_data) = (root.clone(), data);
    let (thread_stop, thread_said) = (Arc::clone(&stop), Arc::clone(&said));
    let (armed, first_armed) = std::sync::mpsc::channel();
    let watcher = thread::spawn(move || {
        let guard = TestGuard(GitGuard::new(&thread_root, &thread_data, &matter_one()));
        let session = TestSession {
            stop: &thread_stop,
            said: thread_said,
        };
        let first = DirWatch::arm(&thread_root, &thread_stop);
        assert!(first.is_ok(), "the watch arms");
        armed.send(()).unwrap();
        watch(
            first,
            || DirWatch::arm(&thread_root, &thread_stop),
            &guard,
            &session,
        );
    });
    first_armed.recv().unwrap();

    rename_new(&root.join("Precedents"), &root.join(".git")).unwrap();
    let tick = Stop::new().unwrap();
    let gone = (0..100).any(|_| !tick.wait(Duration::from_millis(20)) && !there(&root, ".git"));
    stop.set();
    watcher.join().unwrap();

    assert!(gone, "set aside within 2 s");
    let said = said.lock().unwrap();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("Clauseo set aside .git: an agent can't create a git repository"));
}

/// Whether `name` is in `root`, itself.
fn there(root: &Path, name: &str) -> bool {
    std::fs::symlink_metadata(root.join(name)).is_ok()
}
