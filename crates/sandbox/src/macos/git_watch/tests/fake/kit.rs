//! The fakes' kit beside the file system: a state store in memory, a sink, a session, and the
//! helpers that make and check a watch of `/m`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use super::super::super::aside::Aside;
use super::super::super::state::{MatterState, Store};
use super::super::super::{Asks, Event, Launch, Phase, Session, Watch, flags};
use super::{FakeFs, MATTER, SET_ASIDE};
use crate::{Denial, DenialSink, GitItem, GitOutcome};

/// A matter state in memory.
#[derive(Clone, Default)]
pub(in super::super) struct MemStore(pub(in super::super) Arc<Mutex<MatterState>>);

impl Store for MemStore {
    fn update<T>(&self, change: impl FnOnce(&mut MatterState) -> T) -> io::Result<T> {
        Ok(change(&mut self.0.lock().unwrap()))
    }
}

/// The notices the watch sent.
#[derive(Default)]
pub(in super::super) struct Notices(Mutex<Vec<Denial>>);

impl DenialSink for Notices {
    fn denied(&self, d: Denial) {
        self.0.lock().unwrap().push(d);
    }
}

impl Notices {
    pub(in super::super) fn taken(&self) -> Vec<Denial> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }

    pub(in super::super) fn count(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

/// A session whose processes go when told, or when ended (if `dies`).
#[derive(Clone, Default)]
pub(in super::super) struct FakeSession {
    pub(in super::super) gone: Arc<AtomicBool>,
    pub(in super::super) ends: Arc<AtomicU32>,
    pub(in super::super) dies: bool,
}

impl Session for FakeSession {
    fn gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }

    fn end(&self) {
        self.ends.fetch_add(1, Ordering::SeqCst);
        if self.dies {
            self.gone.store(true, Ordering::SeqCst);
        }
    }
}

static NEXT_SESSION: AtomicU32 = AtomicU32::new(0);

/// The aside of a watch of `/m` for a new session.
pub(in super::super) fn aside(notices: &Arc<Notices>) -> Aside {
    let n = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    Aside::new(
        PathBuf::from(MATTER),
        PathBuf::from(SET_ASIDE),
        format!("clauseo-1-0-{n}"),
        Arc::clone(notices) as Arc<dyn DenialSink>,
    )
}

/// A watch of `/m`; `git` are the launch's `.git`s.
pub(in super::super) fn watch(fs: &FakeFs, git: &[&str]) -> (Watch<FakeFs>, Arc<Notices>) {
    watch_with(fs, git, &[])
}

/// [`watch`] with the launch's links out too.
pub(in super::super) fn watch_with(
    fs: &FakeFs,
    git: &[&str],
    links: &[&str],
) -> (Watch<FakeFs>, Arc<Notices>) {
    let paths = |p: &[&str]| p.iter().map(PathBuf::from).collect();
    watch_launch(
        fs,
        &Launch {
            git: paths(git),
            links: paths(links),
            keep_repos: true,
            ..Launch::default()
        },
    )
}

/// [`watch`] with the launch given whole.
pub(in super::super) fn watch_launch(
    fs: &FakeFs,
    launch: &Launch,
) -> (Watch<FakeFs>, Arc<Notices>) {
    let notices = Arc::new(Notices::default());
    let watch = Watch::new(fs.clone(), PathBuf::from(MATTER), aside(&notices), launch);
    (watch, notices)
}

pub(in super::super) fn event(path: &str, flags: u32) -> Event {
    Event {
        path: PathBuf::from(path),
        flags,
    }
}

/// A check that found nothing: it does not ask to end the session.
pub(in super::super) fn quiet(asks: Asks) {
    assert!(!asks.end, "{asks:?}");
}

/// `events` find something while the agent may run: nothing moves, and the session must end.
/// Then, the session ended, the final scan sets aside.
pub(in super::super) fn found(watch: &mut Watch<FakeFs>, events: &[Event]) {
    let asks = watch.events(events);
    assert!(asks.end, "{events:?}");
    watch.ended_session();
    quiet(watch.full_scan(Phase::Ended));
}

/// No notice came.
pub(in super::super) fn none(notices: &Notices) {
    assert_eq!(notices.taken(), Vec::<Denial>::new());
}

pub(in super::super) const MOVED_IN: u32 = flags::ITEM_RENAMED | 0x2_0000;

/// The one notice, for the `.git` at `rel`, moved into the set-aside folder.
pub(in super::super) fn assert_moved(notices: &Notices, rel: &str) {
    assert_set_aside(notices, rel, GitItem::DotGit);
}

/// The one notice, for the `want` at `rel`, moved into the set-aside folder.
pub(in super::super) fn assert_set_aside(notices: &Notices, rel: &str, want: GitItem) {
    let notices = notices.taken();
    assert_eq!(notices.len(), 1, "{notices:?}");
    let Denial::GitSetAside {
        path,
        what,
        outcome: GitOutcome::Moved(to),
        ..
    } = &notices[0]
    else {
        panic!("{notices:?}");
    };
    assert_eq!((path.as_path(), *what), (Path::new(rel), want));
    assert!(to.starts_with(SET_ASIDE) && to.ends_with("item"), "{to:?}");
}
