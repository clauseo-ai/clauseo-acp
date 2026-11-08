//! Starting a session's watches: the matter's, and the notes folder's, each with its own
//! stream, state file and thread, all ended by the one session.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};

use super::super::aside::Aside;
use super::super::host::{self, LIVE};
use super::super::state::{self, FileStore, SessionRecord, Store};
use super::super::{Fs, Launch, Watch, fsevents, os};
use super::watchdog::{self, Busy};
use super::{CANARY, End, Flow, Mode, Msg, Runner, Session, TICK};
use crate::macos::jail_procs::{self, Probe};
use crate::{Clock, DenialSink, Policy, SystemClock};

/// A session's watches, started before its agent. [`Watching::attach`] hands them the agent
/// once it runs; dropped without that, they say the agent did not start, and each scans once
/// and stops.
pub(crate) struct Watching {
    parts: Vec<Part>,
}

/// One watched folder's thread.
struct Part {
    tx: Option<Sender<Msg>>,
    busy: Arc<Busy>,
}

impl std::fmt::Debug for Watching {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watching")
            .field("folders", &self.parts.len())
            .finish()
    }
}

impl Watching {
    /// These watches and `other`'s, ended by one session.
    pub(crate) fn and(mut self, mut other: Watching) -> Watching {
        self.parts.append(&mut other.parts);
        self
    }

    /// The agent runs: each watch now ends with its session. A watch whose thread is gone
    /// ends the session at once (fail-closed), as does one the watchdog already gave up on.
    pub(crate) fn attach(mut self, session: impl Session + 'static) {
        let session: Arc<dyn Session> = Arc::new(session);
        for part in &mut self.parts {
            part.busy.attach(&session);
            if let Some(tx) = part.tx.take()
                && tx.send(Msg::Attach(Arc::clone(&session))).is_err()
            {
                tracing::error!("the git watch's thread is gone; ending the session");
                session.end();
            }
        }
    }
}

impl Drop for Watching {
    fn drop(&mut self) {
        for part in &mut self.parts {
            if let Some(tx) = part.tx.take()
                && tx.send(Msg::Abandon).is_err()
            {
                tracing::error!("the git watch's thread is gone");
            }
        }
    }
}

/// Starts the watches of `policy`'s matter and notes folder for `session` (an id for the
/// records) with `probe`, before the agent, in `mode`: the set-aside folders are made, each
/// folder's state is checked (refused while an item an earlier scan could not handle is
/// there, or while a process of an earlier session outlives its kill; a dead session's final
/// scan run), the launch's items noted and the session recorded, and each stream started.
/// Refused without a set-aside folder, when one can't be made, or when a stream can't start:
/// the agent never runs without the backstop.
pub(crate) fn start(
    policy: &Policy,
    session: &str,
    probe: &Probe,
    notices: &Arc<dyn DenialSink>,
    mode: Mode,
) -> std::io::Result<Watching> {
    let data = policy
        .git_set_aside()
        .ok_or_else(|| std::io::Error::other("the policy names no git set-aside folder"))?;
    os::Real.make_dirs(data)?;
    let folder = match policy.volume_set_aside() {
        Some(dir) => os::volume_folder(dir)?,
        None => data.to_owned(),
    };
    let root = Root {
        dir: policy.matter(),
        folder,
        launch: Launch::of(policy),
        session: session.to_owned(),
    };
    let matter = start_root(root, data, probe, notices, mode)?;
    let Some(notes) = policy.notes() else {
        return Ok(matter);
    };
    // The notes folder is the data dir's: the data dir's set-aside folder takes its items.
    let root = Root {
        dir: notes,
        folder: data.to_owned(),
        launch: Launch::default(),
        session: format!("{session}-notes"),
    };
    Ok(matter.and(start_root(root, data, probe, notices, mode)?))
}

/// One folder a session's watch covers.
struct Root<'a> {
    dir: &'a Path,
    /// Its set-aside folder, on its volume.
    folder: PathBuf,
    launch: Launch,
    /// The watch's id in the records and the host's register.
    session: String,
}

/// [`start`] for one folder, its state kept under `data`.
fn start_root(
    root: Root<'_>,
    data: &Path,
    probe: &Probe,
    notices: &Arc<dyn DenialSink>,
    mode: Mode,
) -> std::io::Result<Watching> {
    let Root {
        dir,
        folder,
        launch,
        session,
    } = root;
    let store = FileStore::new(data, dir);
    let this = jail_procs::this_process();
    state::begin(&store, &os::Real, dir, notices, &host::host())?;
    let canary = os::Real
        .resolve(&folder)
        .unwrap_or_else(|| folder.clone())
        .join(CANARY);
    let aside = Aside::new(
        dir.to_owned(),
        folder.clone(),
        session.clone(),
        Arc::clone(notices),
    );
    let watch = Watch::new(os::Real, dir.to_owned(), aside, &launch);
    let kill = probe.clone();
    LIVE.add(
        &session,
        Arc::new(move || {
            kill.kill_all();
        }),
    )?;
    let launched = os::Real
        .now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_micros()).ok());
    let record = SessionRecord {
        session: session.clone(),
        host_pid: this.0,
        host_start: this.1,
        probe: probe.name().to_owned(),
        set_aside: folder.as_path().into(),
        baseline: watch.baseline().clone(),
        launched,
        outlived: false,
        finds: Vec::new(),
    };
    let started = state::record(&store, record).and_then(|()| {
        start_with(
            watch,
            session.clone(),
            store,
            Some(canary),
            mode,
            fsevents::start,
        )
    });
    if started.is_err() {
        LIVE.remove(&session);
    }
    started
}

/// Ends the session if the thread dies (fail-closed), and takes it off the host's register
/// whatever way the thread ends.
struct Guard {
    busy: Arc<Busy>,
    session: String,
}

impl Drop for Guard {
    fn drop(&mut self) {
        LIVE.remove(&self.session);
        if std::thread::panicking() {
            tracing::error!("the git watch's thread panicked; ending the session");
            if let Some(session) = self.busy.session() {
                session.end();
            }
        }
    }
}

/// [`start`] for one folder with the watch made and the stream's source given: the tests pass
/// a fake one and a store in memory. The source watches the folder, and the canary's folder
/// when it is outside it; [`Mode::AfterOnly`] never calls it. When the source fails, the
/// launch is refused and the session's record, if `store` has one, removed.
pub(crate) fn start_with<F, S, T>(
    watch: Watch<F>,
    session: String,
    store: T,
    canary: Option<PathBuf>,
    mode: Mode,
    source: impl FnOnce(&[PathBuf], Sender<Msg>) -> Result<S, String>,
) -> std::io::Result<Watching>
where
    F: Fs + 'static,
    S: Send + 'static,
    T: Store,
{
    let (tx, rx) = mpsc::channel();
    let matter = watch.matter().to_owned();
    let mut roots = vec![matter.clone()];
    roots.extend(
        canary
            .as_deref()
            .and_then(Path::parent)
            .filter(|folder| !folder.starts_with(&matter))
            .map(Path::to_owned),
    );
    let clock = SystemClock::new();
    let now = clock.now();
    let mut runner = Runner::new(watch, mode, now);
    if let Some(canary) = canary {
        runner = runner.with_canary(canary, now);
    }
    let busy = Arc::clone(runner.busy());
    let refuse = |error: std::io::Error| {
        state::end(&store, &session, Vec::new());
        Err(error)
    };
    if let Err(error) = watchdog::watch(&busy) {
        return refuse(error);
    }
    let stream = match mode {
        Mode::AfterOnly => None,
        Mode::Live => match source(&roots, tx.clone()) {
            Ok(stream) => Some(stream),
            Err(error) => {
                tracing::error!(
                    matter = %matter.display(), %error,
                    "the git watch's FSEvents stream did not start; refusing the launch"
                );
                return refuse(std::io::Error::other(format!(
                    "Clauseo won't start the AI tool: it can't watch {} for changes ({error}).",
                    matter.display()
                )));
            }
        },
    };
    // The thread holds a sender of its own: without a stream, the launch's is the only other
    // one, and `Watching::attach` drops it. The end comes from the session, or an abandon.
    let own = tx.clone();
    let guard_busy = Arc::clone(&busy);
    std::thread::Builder::new()
        .name("clauseo-git-watch".to_owned())
        .spawn(move || {
            let _own = own;
            let _guard = Guard {
                busy: guard_busy,
                session: session.clone(),
            };
            drive(&mut runner, &rx, &clock);
            drop(stream);
            match runner.finish() {
                End::Scanned(stuck) => state::end(&store, &session, stuck),
                End::Outlived(finds) => state::outlived(&store, &session, &finds),
            }
            tracing::debug!(matter = %matter.display(), "the git watch stopped");
        })?;
    Ok(Watching {
        parts: vec![Part { tx: Some(tx), busy }],
    })
}

/// Feeds `runner` until it is done. Every sender gone counts as an abandon.
fn drive<F: Fs>(runner: &mut Runner<F>, rx: &Receiver<Msg>, clock: &dyn Clock) {
    loop {
        let msg = match rx.recv_timeout(TICK) {
            Ok(msg) => Some(msg),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Msg::Abandon),
        };
        if runner.input(msg, clock.now()) == Flow::Done {
            return;
        }
    }
}
