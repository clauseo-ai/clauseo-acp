//! A matter job that panics: that job is answered, the matter opens again for the next one, a
//! half-taken checkpoint leaves the next one whole, and repeated panics make the worker rest.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use clauseo_matters::{CheckpointOutcome, DEFAULT_MARKER, NewMatter, Settings, TurnMeta};
use clauseo_protocol::{CheckpointId, ErrorCode, MatterId, ThreadId, Timestamp, TurnId};
use uuid::Uuid;

use super::{Crashes, LIMIT, REST, WINDOW, failed_in_history, hook};
use crate::agent::data_fence::PathsChecked;
use crate::agent::folder_support::CheckSource;
use crate::error::Error;
use crate::matter_worker::{MatterJob, MatterReply, MatterWorker, matter_settings};
use crate::pipeline::tests::support::TempDir;
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

fn meta(n: u32) -> TurnMeta {
    TurnMeta {
        checkpoint: CheckpointId::new(v7(n)).unwrap(),
        thread: ThreadId::new(v7(90)).unwrap(),
        turn: TurnId::new(v7(91)).unwrap(),
        at: Timestamp::new(1_790_000_000_000).unwrap(),
    }
}

fn after_turn(n: u32) -> MatterJob {
    MatterJob::AfterTurn {
        meta: meta(n),
        tool_paths: TurnPaths::default(),
        before: None,
    }
}

/// A worker of matter `id` with `settings`, its replies on the returned channel.
struct Worker {
    worker: MatterWorker<u32>,
    replies: mpsc::Receiver<MatterReply>,
}

impl Worker {
    fn start(id: MatterId, settings: Settings) -> Self {
        let (tx, replies) = mpsc::channel();
        let post = Box::new(move |_tag, reply| tx.send(reply).unwrap());
        let clock: Arc<dyn crate::clock::Clock> = Arc::new(crate::clock::SystemClock);
        let worker = MatterWorker::<u32>::start((id, clock), settings, post).unwrap();
        Self { worker, replies }
    }

    /// Sends `job` and waits for its reply.
    fn ask(&self, job: MatterJob) -> MatterReply {
        self.worker.send(job, 0).unwrap();
        self.replies
            .recv_timeout(PATIENCE)
            .expect("the job was answered")
    }

    /// Creates the matter in `folder`.
    fn create(&self, id: MatterId, folder: &Path) {
        let reply = self.ask(MatterJob::Create {
            new: NewMatter {
                id,
                folder: folder.to_owned(),
                created_at: Timestamp::new(1_790_000_000_000).unwrap(),
                marker: DEFAULT_MARKER,
                checkpoint: CheckpointId::new(v7(1)).unwrap(),
            },
            check: CheckSource::Windows,
        });
        assert!(matches!(reply, MatterReply::Created(Ok(_))), "{reply:?}");
    }
}

/// The error a failed reply holds, by its code.
fn internal(reply: &MatterReply) -> bool {
    let (MatterReply::BeforeTurn(Err(error))
    | MatterReply::AfterTurn(Err(error))
    | MatterReply::Papers(Err(error))) = reply
    else {
        panic!("not a failure: {reply:?}")
    };
    error.code() == ErrorCode::Internal
}

#[test]
fn a_job_that_panics_is_answered_and_the_next_job_on_the_matter_runs() {
    let dir = TempDir::new("crash-next-job");
    let (id, folder) = (MatterId::new(v7(2)).unwrap(), dir.path().join("matter"));
    let worker = Worker::start(id, matter_settings(&dir.path().join("data")));
    worker.create(id, &folder);

    hook::panic_next(id, "BeforeTurn", 1);
    let failed = worker.ask(MatterJob::BeforeTurn(meta(3)));
    assert!(internal(&failed), "{failed:?}");

    // The same worker, the matter opened again from its folder.
    assert!(worker.worker.is_alive());
    let again = worker.ask(MatterJob::BeforeTurn(meta(4)));
    assert!(matches!(again, MatterReply::BeforeTurn(Ok(_))), "{again:?}");
    write::write_file(&folder.join("notes.txt"), b"Alpha\n").unwrap();
    let MatterReply::AfterTurn(Ok(after)) = worker.ask(after_turn(5)) else {
        panic!("the checkpoint after the panic failed")
    };
    assert_eq!(
        after.checkpoint.outcome,
        CheckpointOutcome::Created(meta(5).checkpoint)
    );
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
    let dir = TempDir::new("crash-mid-checkpoint");
    let (id, folder) = (MatterId::new(v7(2)).unwrap(), dir.path().join("matter"));
    let data: PathBuf = dir.path().join("data");
    let probe = Arc::new(PanicProbe::default());
    let settings = Settings {
        probe: Arc::<PanicProbe>::clone(&probe),
        ..matter_settings(&data)
    };
    let worker = Worker::start(id, settings);
    worker.create(id, &folder);
    write::write_file(&folder.join("a.txt"), b"Alpha\n").unwrap();
    write::write_file(&folder.join("b.txt"), b"Beta\n").unwrap();

    probe.0.store(true, Ordering::SeqCst);
    let failed = worker.ask(after_turn(3));
    assert!(internal(&failed), "{failed:?}");
    probe.0.store(false, Ordering::SeqCst);

    let MatterReply::AfterTurn(Ok(after)) = worker.ask(after_turn(4)) else {
        panic!("the checkpoint after the panic failed")
    };
    assert_eq!(
        after.checkpoint.outcome,
        CheckpointOutcome::Created(meta(4).checkpoint)
    );
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
    let fresh = Worker::start(id, matter_settings(&data));
    let opened = fresh.ask(MatterJob::Open {
        id,
        folder,
        relink: None,
    });
    let MatterReply::Opened(Ok(last)) = opened else {
        panic!("{opened:?}")
    };
    assert_eq!(last, Some(meta(4).checkpoint));
    fresh.worker.stop().unwrap();
}

#[test]
fn a_held_open_check_that_panics_reads_as_nothing_held() {
    let dir = TempDir::new("crash-held-open");
    let id = MatterId::new(v7(2)).unwrap();
    let worker = Worker::start(id, matter_settings(&dir.path().join("data")));
    hook::panic_next(id, "HeldOpen", 1);
    let reply = worker.ask(MatterJob::HeldOpen {
        folder: dir.path().to_owned(),
        paths: vec![dir.path().join("brief.docx")],
        fence: None,
    });
    assert!(
        matches!(reply, MatterReply::HeldOpen(PathsChecked::Held(None))),
        "{reply:?}"
    );
    worker.worker.stop().unwrap();
}

#[test]
fn panics_in_a_row_make_the_worker_rest() {
    let dir = TempDir::new("crash-rest");
    let (id, folder) = (MatterId::new(v7(2)).unwrap(), dir.path().join("matter"));
    let worker = Worker::start(id, matter_settings(&dir.path().join("data")));
    worker.create(id, &folder);
    hook::panic_next(id, "ListPapers", LIMIT);
    for _ in 0..LIMIT {
        let failed = worker.ask(MatterJob::ListPapers { limit: 10 });
        assert!(internal(&failed), "{failed:?}");
    }
    // Nothing is armed now, so this listing would run; the worker rests and refuses it.
    let resting = worker.ask(MatterJob::ListPapers { limit: 10 });
    assert!(internal(&resting), "{resting:?}");
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
    let history = || {
        Error::Matters(clauseo_matters::Error::History {
            op: "publish",
            source: "the ref moved".into(),
        })
    };
    assert!(failed_in_history(&MatterReply::AfterTurn(Err(history()))));
    assert!(failed_in_history(&MatterReply::Undone(Err(history()))));
    assert!(!failed_in_history(&MatterReply::Compared(Err(history()))));
    let locked = Error::Matters(clauseo_matters::Error::FileLocked { paths: Vec::new() });
    assert!(!failed_in_history(&MatterReply::BeforeTurn(Err(locked))));
}
