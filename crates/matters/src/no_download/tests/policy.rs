//! macOS: the guard sets the calling thread's policy for dataless files off and puts back what
//! the thread had, on return, on error and while unwinding; and every read of the folder runs
//! under it. Each test runs on a thread of its own, so no test sees another's policy.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::super::macos::{
    IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT as DEFAULT,
    IOPOL_MATERIALIZE_DATALESS_FILES_OFF as OFF, IOPOL_MATERIALIZE_DATALESS_FILES_ON as ON,
    process_policy, set_thread_policy, thread_policy,
};
use super::super::{NoDownload, fault};
use clauseo_protocol::{FileChoice, FileDecision};

use super::{Fixture, Local, at, cid, meta, path, thread, turn};
use crate::types::{CompareSide, Marker, NewMatter, Restore, RestoreFiles, UndoRequest};
use crate::{Matter, PlaceholderProbe, write};

fn on_own_thread(test: impl FnOnce() + Send + 'static) {
    std::thread::spawn(test).join().unwrap();
}

#[test]
fn the_guard_turns_downloads_off_and_puts_back_on() {
    on_own_thread(|| {
        set_thread_policy(ON).unwrap();
        {
            let _guard = NoDownload::begin(&Local);
            assert_eq!(thread_policy().unwrap(), OFF);
        }
        assert_eq!(thread_policy().unwrap(), ON);
    });
}

#[test]
fn the_guard_puts_back_default_so_the_thread_follows_the_process_again() {
    on_own_thread(|| {
        assert_eq!(thread_policy().unwrap(), DEFAULT);
        drop(NoDownload::begin(&Local));
        assert_eq!(thread_policy().unwrap(), DEFAULT);
    });
}

#[test]
fn nested_guards_put_back_the_outer_policy() {
    on_own_thread(|| {
        set_thread_policy(ON).unwrap();
        let outer = NoDownload::begin(&Local);
        {
            let _inner = NoDownload::begin(&Local);
            assert_eq!(thread_policy().unwrap(), OFF);
        }
        assert_eq!(thread_policy().unwrap(), OFF);
        drop(outer);
        assert_eq!(thread_policy().unwrap(), ON);
    });
}

#[test]
fn the_guard_puts_the_policy_back_while_unwinding() {
    on_own_thread(|| {
        set_thread_policy(ON).unwrap();
        let unwound = catch_unwind(|| {
            let _guard = NoDownload::begin(&Local);
            panic!("a read panicked");
        });
        assert!(unwound.is_err());
        assert_eq!(thread_policy().unwrap(), ON);
    });
}

#[test]
fn the_guard_changes_only_its_own_thread() {
    on_own_thread(|| {
        let process = process_policy().unwrap();
        let _guard = NoDownload::begin(&Local);
        assert_eq!(process_policy().unwrap(), process);
        let other = std::thread::spawn(|| thread_policy().unwrap())
            .join()
            .unwrap();
        assert_eq!(other, DEFAULT);
    });
}

#[test]
fn a_policy_the_system_does_not_know_is_an_error() {
    on_own_thread(|| {
        assert!(set_thread_policy(99).is_err());
    });
}

#[test]
fn every_read_of_every_entry_point_runs_with_downloads_off_and_the_caller_keeps_its_policy() {
    on_own_thread(|| {
        set_thread_policy(ON).unwrap();
        let fx = Fixture::new(
            "no-download-policy",
            &[("a.txt", b"one"), ("sub/b.txt", b"two")],
        );
        let check = |what: &str| {
            let seen = fault::take_policies();
            assert!(!seen.is_empty(), "{what} read nothing");
            assert!(seen.iter().all(|policy| *policy == OFF), "{what}: {seen:?}");
            assert_eq!(thread_policy().unwrap(), ON, "{what}");
        };

        let created = Matter::create(
            fx.settings.clone(),
            NewMatter {
                id: "01910000-0000-7000-8000-000000000002".parse().unwrap(),
                folder: fx.folder.parent().unwrap().join("created"),
                created_at: at(1),
                marker: Marker::None,
                checkpoint: cid(1),
            },
        );
        created.unwrap();
        check("create");

        // Before the import: a scan refuses a folder that is a matter already.
        crate::scan(
            &fx.folder,
            &[],
            &fx.settings,
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        check("scan");

        let (mut matter, _) = fx.import();
        check("import");
        write::append(&fx.file("a.txt"), b" before").unwrap();
        matter.before_turn(&meta(2)).unwrap();
        check("before_turn");
        write::append(&fx.file("a.txt"), b" agent").unwrap();
        matter
            .after_turn(&meta(3), &std::collections::BTreeSet::new())
            .unwrap();
        check("after_turn");
        write::append(&fx.file("a.txt"), b" lawyer").unwrap();
        matter.edited_since(cid(2), cid(3)).unwrap();
        check("edited_since");
        matter.preview_restore(cid(1), RestoreFiles::All).unwrap();
        check("preview_restore");
        matter
            .compare(CompareSide::Checkpoint(cid(1)), CompareSide::Folder)
            .unwrap();
        check("compare");

        let decisions = [FileDecision {
            path: path("a.txt"),
            choice: FileChoice::Undo,
        }];
        matter
            .undo(&UndoRequest {
                before: cid(2),
                after: cid(3),
                decisions: &decisions,
                thread: Some(thread()),
                turn: Some(turn(3)),
                outside_edits: cid(4),
                checkpoint: cid(5),
                at: at(5),
            })
            .unwrap();
        check("undo");
        write::append(&fx.file("a.txt"), b" again").unwrap();
        matter
            .restore(&Restore {
                from: cid(1),
                files: RestoreFiles::All,
                outside_edits: cid(6),
                checkpoint: cid(7),
                at: at(7),
            })
            .unwrap();
        check("restore");
    });
}

#[test]
fn an_entry_point_that_fails_puts_the_policy_back() {
    on_own_thread(|| {
        set_thread_policy(ON).unwrap();
        let fx = Fixture::new("no-download-policy-error", &[("a.txt", b"one")]);
        let (mut matter, _) = fx.import();
        write::append(&fx.file("a.txt"), b" two").unwrap();
        fault::fail(&fx.file("a.txt"), 0, std::io::ErrorKind::PermissionDenied);
        assert!(matter.before_turn(&meta(2)).is_err());
        assert_eq!(thread_policy().unwrap(), ON);
        fault::clear();
    });
}

/// Panics when asked once armed.
struct Panicking(AtomicBool);

impl PlaceholderProbe for Panicking {
    fn is_cloud_only(&self, _path: &std::path::Path, _metadata: &std::fs::Metadata) -> bool {
        assert!(
            !self.0.load(Ordering::Relaxed),
            "the probe panicked inside the walk"
        );
        false
    }
}

#[test]
fn an_entry_point_that_panics_puts_the_policy_back() {
    on_own_thread(|| {
        set_thread_policy(ON).unwrap();
        let probe = Arc::new(Panicking(AtomicBool::new(false)));
        let mut fx = Fixture::new("no-download-policy-panic", &[("a.txt", b"one")]);
        fx.settings.probe = Arc::<Panicking>::clone(&probe);
        let (mut matter, _) = fx.import();
        probe.0.store(true, Ordering::Relaxed);
        let unwound = catch_unwind(AssertUnwindSafe(|| matter.before_turn(&meta(2))));
        assert!(unwound.is_err());
        assert_eq!(thread_policy().unwrap(), ON);
    });
}
