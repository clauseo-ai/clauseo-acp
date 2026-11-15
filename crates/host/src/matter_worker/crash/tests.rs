//! A matter job that panics: that job is answered, the matter opens again for the next one, a
//! half-taken checkpoint leaves the next one whole, and repeated panics make the worker rest.
//! Each test has its own data folder and matter id, so the armed panics of one never fire in
//! another that runs in the same process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use clauseo_matters::{CheckpointOutcome, DEFAULT_MARKER, NewMatter, Settings, TurnMeta};
use clauseo_protocol::{
    CheckpointId, ErrorCode, MatterId, MatterPath, RestoreRequest, ThreadId, Timestamp, TurnId,
};
use uuid::Uuid;

use super::{Crashes, LIMIT, REST, WINDOW, failed_in_history, hook};
use crate::agent::data_fence::PathsChecked;
use crate::agent::folder_support::CheckSource;
use crate::clock::{Clock, ManualClock, SystemClock};
use crate::error::Error;
use crate::matter_worker::{
    MatterJob, MatterReply, MatterWorker, Registry, RestoreJob, UndoJob, matter_settings,
};
use crate::pipeline::tests::support::TempDir;
use crate::revisions::{Act, RevisionsDone, RevisionsJob};
use crate::sent::TurnPaths;
use crate::write;

/// Long enough for any job here; a reply that never comes fails the test rather than hang it.
const PATIENCE: Duration = Duration::from_secs(60);

/// A valid v7 UUID, distinct per `n`.
fn v7(n: u32) -> Uuid {
    let mut random = [0_u8; 10];
    for (byte, from) in random.iter_mut().zip(n.to_be_bytes()) {
        *byte = from;
    }
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &random).into_uuid()
}

fn id(n: u32) -> CheckpointId {
    CheckpointId::new(v7(n)).unwrap()
}

fn at() -> Timestamp {
    Timestamp::new(1_790_000_000_000).unwrap()
}

fn meta(n: u32) -> TurnMeta {
    TurnMeta {
        checkpoint: id(n),
        thread: ThreadId::new(v7(90)).unwrap(),
        turn: TurnId::new(v7(91)).unwrap(),
        at: at(),
    }
}

fn after_turn(n: u32) -> MatterJob {
    MatterJob::AfterTurn {
        meta: meta(n),
        tool_paths: TurnPaths::default(),
        before: None,
    }
}

/// A test's matter: its id (from the test's own `n`), its folder and its data folder.
struct Case {
    dir: TempDir,
    matter: MatterId,
}

impl Case {
    fn new(name: &str, n: u32) -> Self {
        Self {
            dir: TempDir::new(name),
            matter: MatterId::new(v7(1000 + n)).unwrap(),
        }
    }

    fn folder(&self) -> PathBuf {
        self.dir.path().join("matter")
    }

    fn data(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    /// Where this test's histories live, the key its armed panics fire by.
    fn root(&self) -> PathBuf {
        self.data().join("matters")
    }

    fn worker(&self) -> Worker {
        self.worker_with(matter_settings(&self.data()), Arc::new(SystemClock))
    }

    fn worker_with(&self, settings: Settings, clock: Arc<dyn Clock>) -> Worker {
        let (tx, replies) = mpsc::channel();
        let post = Box::new(move |_tag, reply| tx.send(reply).unwrap());
        let worker = MatterWorker::<u32>::start((self.matter, clock), settings, post).unwrap();
        Worker { worker, replies }
    }
}

/// A worker, its replies on the channel.
struct Worker {
    worker: MatterWorker<u32>,
    replies: mpsc::Receiver<MatterReply>,
}

impl Worker {
    /// Sends `job` and waits for its reply.
    fn ask(&self, job: MatterJob) -> MatterReply {
        self.worker.send(job, 0).unwrap();
        self.replies
            .recv_timeout(PATIENCE)
            .expect("the job was answered")
    }

    /// Creates the case's matter.
    fn create(&self, case: &Case) {
        let reply = self.ask(MatterJob::Create {
            new: NewMatter {
                id: case.matter,
                folder: case.folder(),
                created_at: at(),
                marker: DEFAULT_MARKER,
                checkpoint: id(1),
            },
            check: CheckSource::Windows,
        });
        assert!(matches!(reply, MatterReply::Created(Ok(_))), "{reply:?}");
    }

    /// A before-turn checkpoint, which needs the matter open.
    fn before_turn_works(&self, n: u32) {
        let reply = self.ask(MatterJob::BeforeTurn(meta(n)));
        assert!(matches!(reply, MatterReply::BeforeTurn(Ok(_))), "{reply:?}");
    }
}

/// The error a failed reply holds.
fn error_of(reply: &MatterReply) -> &Error {
    let (MatterReply::BeforeTurn(Err(error))
    | MatterReply::AfterTurn(Err(error))
    | MatterReply::Papers(Err(error))
    | MatterReply::Located(Err(error))
    | MatterReply::Undone(Err(error))
    | MatterReply::Restored(Err(error))) = reply
    else {
        panic!("not a failure: {reply:?}")
    };
    error
}

fn internal(reply: &MatterReply) -> bool {
    error_of(reply).code() == ErrorCode::Internal
}

#[test]
fn a_job_that_panics_is_answered_and_the_next_job_on_the_matter_runs() {
    let case = Case::new("crash-next-job", 1);
    let worker = case.worker();
    worker.create(&case);

    hook::panic_next(&case.root(), "BeforeTurn", 1);
    let failed = worker.ask(MatterJob::BeforeTurn(meta(3)));
    assert!(internal(&failed), "{failed:?}");

    // The same worker, the matter opened again from its folder.
    assert!(worker.worker.is_alive());
    worker.before_turn_works(4);
    write::write_file(&case.folder().join("notes.txt"), b"Alpha\n").unwrap();
    let MatterReply::AfterTurn(Ok(after)) = worker.ask(after_turn(5)) else {
        panic!("the checkpoint after the panic failed")
    };
    assert_eq!(after.checkpoint.outcome, CheckpointOutcome::Created(id(5)));
    worker.worker.stop().unwrap();
}

/// A locate empties the slot before `Matter::locate`; a panic there leaves the matter to be
/// opened again from the folder it had, not stuck with none.
#[test]
fn a_locate_that_panics_leaves_the_matter_to_open_from_its_old_folder() {
    let case = Case::new("crash-locate", 2);
    let worker = case.worker();
    worker.create(&case);
    hook::panic_next(&case.root(), "Locate:emptied", 1);
    let failed = worker.ask(MatterJob::Locate {
        id: case.matter,
        folder: case.dir.path().join("moved"),
        current: case.folder(),
        registry: Registry {
            refused: Vec::new(),
            others: Vec::new(),
        },
    });
    assert!(internal(&failed), "{failed:?}");
    worker.before_turn_works(3);
    worker.worker.stop().unwrap();
}

/// An undo, a restore and a tracked-changes job that panic are each answered, and the next job
/// runs; the tracked-changes job's private folder is removed. The clock passes a window after
/// each panic, so three panics never make the worker rest here.
#[test]
fn an_undo_a_restore_and_a_revisions_job_that_panic_are_answered() {
    let case = Case::new("crash-writers", 3);
    let clock = ManualClock::new(at());
    let worker = case.worker_with(matter_settings(&case.data()), Arc::new(clock.clone()));
    worker.create(&case);

    hook::panic_next(&case.root(), "Undo", 1);
    let undo = worker.ask(MatterJob::Undo(UndoJob {
        before: id(1),
        after: id(1),
        decisions: Vec::new(),
        thread: None,
        turn: None,
        outside_edits: id(4),
        checkpoint: id(5),
        at: at(),
    }));
    assert!(internal(&undo), "{undo:?}");
    worker.before_turn_works(6);
    clock.advance(WINDOW);

    hook::panic_next(&case.root(), "Restore", 1);
    let restore = worker.ask(MatterJob::Restore(RestoreJob {
        from: id(1),
        scope: RestoreRequest::Matter,
        outside_edits: id(7),
        checkpoint: id(8),
        at: at(),
        undoes: None,
    }));
    assert!(internal(&restore), "{restore:?}");
    worker.before_turn_works(9);
    clock.advance(WINDOW);

    let work = case.data().join("revisions").join("job");
    write::create_dir_all(&work).unwrap();
    hook::panic_next(&case.root(), "Revisions", 1);
    let revisions = worker.ask(MatterJob::Revisions(Box::new(RevisionsJob {
        path: MatterPath::new("brief.docx").unwrap(),
        stetdoc: case.dir.path().join("stetdoc"),
        work: work.clone(),
        clock: Arc::new(SystemClock),
        act: Act::List,
    })));
    let MatterReply::Revisions(done) = revisions else {
        panic!("{revisions:?}")
    };
    let RevisionsDone::Listed(Err(error)) = *done else {
        panic!("{done:?}")
    };
    assert_eq!(error.code(), ErrorCode::Internal);
    assert!(!work.exists(), "the private folder is removed");
    worker.before_turn_works(10);
    worker.worker.stop().unwrap();
}

/// A probe that panics on the file `b.txt` while armed: a panic in the middle of a checkpoint's
/// walk, after `a.txt`'s content is stored and before any tree, commit or ref.
#[derive(Debug, Default)]
struct PanicProbe(AtomicBool);

impl clauseo_matters::PlaceholderProbe for PanicProbe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        let armed = self.0.load(Ordering::SeqCst);
        assert!(
            !(armed && path.file_name().is_some_and(|name| name == "b.txt")),
            "a test's panic in a checkpoint's walk"
        );
        false
    }
}

#[test]
fn a_checkpoint_that_panics_half_way_leaves_the_next_one_whole() {
    let case = Case::new("crash-mid-checkpoint", 4);
    let probe = Arc::new(PanicProbe::default());
    let settings = Settings {
        probe: Arc::<PanicProbe>::clone(&probe),
        ..matter_settings(&case.data())
    };
    let worker = case.worker_with(settings, Arc::new(SystemClock));
    worker.create(&case);
    write::write_file(&case.folder().join("a.txt"), b"Alpha\n").unwrap();
    write::write_file(&case.folder().join("b.txt"), b"Beta\n").unwrap();

    probe.0.store(true, Ordering::SeqCst);
    let failed = worker.ask(after_turn(3));
    assert!(internal(&failed), "{failed:?}");
    probe.0.store(false, Ordering::SeqCst);

    let MatterReply::AfterTurn(Ok(after)) = worker.ask(after_turn(4)) else {
        panic!("the checkpoint after the panic failed")
    };
    assert_eq!(after.checkpoint.outcome, CheckpointOutcome::Created(id(4)));
    let mut paths: Vec<_> = after
        .checkpoint
        .changes
        .iter()
        .map(|change| change.path().as_str().to_owned())
        .collect();
    paths.sort();
    assert_eq!(paths, ["a.txt", "b.txt"]);
    worker.worker.stop().unwrap();

    // A fresh open, as at the next start, reads a history whose last checkpoint is the whole
    // one; the half-taken one left nothing on the timeline.
    let fresh = case.worker();
    let opened = fresh.ask(MatterJob::Open {
        id: case.matter,
        folder: case.folder(),
        relink: None,
    });
    let MatterReply::Opened(Ok(last)) = opened else {
        panic!("{opened:?}")
    };
    assert_eq!(last, Some(id(4)));
    fresh.worker.stop().unwrap();
}

/// A held-open check: the agent's file in the case's folder.
fn held_open(case: &Case) -> MatterJob {
    MatterJob::HeldOpen {
        folder: case.folder(),
        paths: vec![case.folder().join("brief.docx")],
        fence: None,
    }
}

fn nothing_held(reply: &MatterReply) -> bool {
    matches!(reply, MatterReply::HeldOpen(PathsChecked::Held(None)))
}

#[test]
fn a_held_open_check_that_panics_reads_as_nothing_held() {
    let case = Case::new("crash-held-open", 5);
    let worker = case.worker();
    hook::panic_next(&case.root(), "HeldOpen", 1);
    let reply = worker.ask(held_open(&case));
    assert!(nothing_held(&reply), "{reply:?}");
    worker.worker.stop().unwrap();
}

/// Three panics make the worker rest on the host clock: a listing is refused at once, the
/// jobs that must not be lost still run, and once the rest is over a listing runs again.
#[test]
fn panics_in_a_row_make_the_worker_rest_until_the_clock_passes_the_rest() {
    let case = Case::new("crash-rest", 6);
    let clock = ManualClock::new(at());
    let worker = case.worker_with(matter_settings(&case.data()), Arc::new(clock.clone()));
    worker.create(&case);
    hook::panic_next(&case.root(), "ListPapers", LIMIT);
    for _ in 0..LIMIT {
        let failed = worker.ask(MatterJob::ListPapers { limit: 10 });
        assert!(internal(&failed), "{failed:?}");
    }
    // Nothing is armed now, so this listing would run; the worker rests and refuses it.
    let resting = worker.ask(MatterJob::ListPapers { limit: 10 });
    assert!(internal(&resting), "{resting:?}");

    // What runs while it rests: a held-open check, and a turn's after-turn checkpoint.
    let held = worker.ask(held_open(&case));
    assert!(nothing_held(&held), "{held:?}");
    write::write_file(&case.folder().join("notes.txt"), b"Alpha\n").unwrap();
    let after = worker.ask(after_turn(3));
    assert!(matches!(after, MatterReply::AfterTurn(Ok(_))), "{after:?}");

    clock.advance(REST);
    let listed = worker.ask(MatterJob::ListPapers { limit: 10 });
    assert!(matches!(listed, MatterReply::Papers(Ok(_))), "{listed:?}");
    worker.worker.stop().unwrap();
}

/// A discard while the worker rests, of a matter it holds unopened, answers `Ok` as ever.
#[test]
fn a_discard_while_resting_is_ok() {
    let case = Case::new("crash-rest-discard", 7);
    let worker = case.worker();
    worker.create(&case);
    hook::panic_next(&case.root(), "ListPapers", LIMIT);
    for _ in 0..LIMIT {
        let failed = worker.ask(MatterJob::ListPapers { limit: 10 });
        assert!(internal(&failed), "{failed:?}");
    }
    let discarded = worker.ask(MatterJob::Discard);
    assert!(
        matches!(discarded, MatterReply::Discarded(Ok(()))),
        "{discarded:?}"
    );
    worker.worker.stop().unwrap();
}

#[test]
fn the_rest_comes_after_enough_panics_within_the_window_and_ends() {
    let at = Duration::from_secs;
    let mut crashes = Crashes::default();
    // Spread wider than the window: never a rest.
    for n in 0..5 {
        assert!(!crashes.crashed(at(n * WINDOW.as_secs())), "panic {n}");
        assert!(!crashes.resting(at(n * WINDOW.as_secs())));
    }
    let mut crashes = Crashes::default();
    for n in 1..LIMIT {
        assert!(!crashes.crashed(at(n as u64)));
    }
    assert!(
        crashes.crashed(at(LIMIT as u64)),
        "the last panic makes it rest"
    );
    let ends = LIMIT as u64 + REST.as_secs();
    assert!(crashes.resting(at(ends - 1)));
    assert!(!crashes.resting(at(ends)), "the rest is over");
    // Counting starts again: one more panic is not a rest.
    assert!(!crashes.crashed(at(ends)));
    assert!(!crashes.resting(at(ends)));
}

#[test]
fn only_a_writing_jobs_history_failure_opens_the_matter_again() {
    let case = Case::new("crash-history-rule", 8);
    let settings = matter_settings(&case.data());
    let history = || {
        Error::Matters(clauseo_matters::Error::History {
            op: "publish",
            source: "the ref moved".into(),
        })
    };
    let io = |path: PathBuf| {
        Error::Matters(clauseo_matters::Error::Io {
            op: "write",
            path,
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        })
    };
    let failed = |reply| failed_in_history(&settings, &reply);
    assert!(failed(MatterReply::AfterTurn(Err(history()))));
    assert!(failed(MatterReply::Undone(Err(history()))));
    assert!(!failed(MatterReply::Compared(Err(history()))));
    // An I/O error in the history reopens; one on a paper does not.
    let ref_file = case.root().join("matter").join("refs");
    assert!(failed(MatterReply::AfterTurn(Err(io(ref_file)))));
    let paper = case.folder().join("brief.docx");
    assert!(!failed(MatterReply::Restored(Err(io(paper)))));
    let locked = Error::Matters(clauseo_matters::Error::FileLocked { paths: Vec::new() });
    assert!(!failed(MatterReply::BeforeTurn(Err(locked))));
}
