//! The in-app folder chooser, for a Windows machine whose own folder dialog would stall on a
//! share that is down (`stall.rs`). `pick_folder_or_choose` opens a session ([`Chooser::open`]);
//! the page asks where it may start ([`Chooser::starts`]), then moves through folders the
//! desktop listed, by index only, or to a path the lawyer typed ([`Chooser::list`]), and
//! `chooser_choose` ends in a native confirm before the folder goes through the bridge's own
//! `pick_folder` (`choose.rs`), so admission is unchanged.
//!
//! Nothing waits between the click and the window: the pre-check starts with the app
//! ([`Chooser::warm`]) and its answer is kept for the run, and a session opens at once; its drives
//! and the servers in use are read once on a thread as it opens, under `listing::DRIVES_WAIT`,
//! and its start points and listings wait for that read (`pending.rs`).
//!
//! Limits: at most [`SESSIONS_MAX`] sessions a run and [`SESSIONS_PER_MINUTE`] a minute (past
//! that the pick draws the native dialog), one at a time (a new one closes the old), at most
//! [`LISTINGS_MAX`] listings in it, closed after [`IDLE_MAX`] idle, on choose and on cancel.
//! Every read but the drives' runs on a thread of its own under `listing::LIST_WAIT`. A typed
//! share opens only on a server this machine already uses (`Place::share_allowed`), a typed path
//! goes through no link, hidden or system folder, and network folders are reachable only under
//! the share or network drive typed last. The start points are the fixed and removable drives,
//! home, the Desktop and the last pick's folder, never Documents, the Libraries or a network place.

mod choose;
mod listing;
mod pending;
mod place;
mod stall;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;
mod wire;

use std::hash::{BuildHasher as _, RandomState};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clauseo_protocol::{ErrorCode, HostError};

pub(crate) use choose::{NativeFolderConfirm, choose_folder, pick_fixed};
use listing::{DRIVES_WAIT, Drives, LIST_WAIT, Machine, bounded, folders, on_network};
use pending::{HAND_IN, Pending};
use place::Place;
use stall::asks_precheck;
use stall::{KnownFolders, PROBE_WAIT, Probes};
pub(crate) use stall::{PICKER_VAR, Route, route};

use crate::bridge::lock;
pub(crate) use wire::{
    ChooserStart, Go, Item, ListState, Listing, PickAnswer, StartKind, StartPoint,
};

/// The most listings one session gives.
pub(crate) const LISTINGS_MAX: u32 = 300;

/// How long a session lasts with no call.
pub(crate) const IDLE_MAX: Duration = Duration::from_secs(600);

/// The most sessions one run opens; past it, or past [`SESSIONS_PER_MINUTE`], a pick draws the
/// native dialog, which the lawyer sees, so page script cannot read folders without end.
pub(crate) const SESSIONS_MAX: usize = 20;

/// The most sessions opened in any minute.
pub(crate) const SESSIONS_PER_MINUTE: usize = 3;

/// The chooser's state, managed by the app: the one session, and what it asks the machine.
#[derive(Clone)]
pub(crate) struct Chooser {
    machine: Arc<dyn Machine>,
    known: Option<Arc<dyn KnownFolders>>,
    probes: Arc<Probes>,
    /// The pre-check's answer for this run, on its way or landed ([`Chooser::warm`]).
    precheck: Arc<Mutex<Option<Arc<Pending<bool>>>>>,
    session: Arc<Mutex<Option<Session>>>,
    outstanding: Arc<AtomicUsize>,
    /// When each session of this run opened.
    opened: Arc<Mutex<Vec<Instant>>>,
}

/// The live session.
struct Session {
    id: u32,
    /// The drives read, on its way from when the session opened (`DRIVES_WAIT`).
    drives: Arc<Pending<Drives>>,
    /// Home and the last pick as the OS gave them; start points once the drives landed.
    asked: (Option<PathBuf>, Option<PathBuf>),
    /// What the drives decide, once they landed.
    ready: Option<Ready>,
    refused: Vec<Place>,
    current: Option<Place>,
    listing: Vec<Item>,
    network_root: Option<Place>,
    listings: u32,
    used: Instant,
}

/// A session's drives, and the start points and home they leave.
#[derive(Clone)]
struct Ready {
    drives: Drives,
    starts: Vec<(StartKind, Place)>,
    home: Option<Place>,
}

impl Chooser {
    /// The chooser of this OS: Windows' drives, folders and known folders; elsewhere a machine
    /// with none, since only Windows opens a session.
    pub(crate) fn system() -> Self {
        #[cfg(windows)]
        return Self::with(Arc::new(windows::System), Some(Arc::new(windows::System)));
        #[cfg(not(windows))]
        Self::with(Arc::new(NoMachine), None)
    }

    /// [`Chooser::system`], its pre-check started at once when a pick would ask it: on Windows,
    /// with no `CLAUSEO_FOLDER_PICKER` choice and, in the e2e build, no fixed folder.
    pub(crate) fn started() -> Self {
        let chooser = Self::system();
        #[cfg(feature = "e2e")]
        let fixed = crate::e2e::matter_dir().is_some();
        #[cfg(not(feature = "e2e"))]
        let fixed = false;
        let setting = std::env::var(PICKER_VAR).ok();
        if asks_precheck(fixed, cfg!(windows), setting.as_deref()) {
            chooser.warm();
        }
        chooser
    }

    /// A chooser asking `machine`, and `known` for the pre-check.
    fn with(machine: Arc<dyn Machine>, known: Option<Arc<dyn KnownFolders>>) -> Self {
        Self {
            machine,
            known,
            probes: Arc::default(),
            precheck: Arc::default(),
            session: Arc::default(),
            outstanding: Arc::default(),
            opened: Arc::default(),
        }
    }

    /// Starts the pre-check on a thread of its own, at most `PROBE_WAIT`, so the first pick finds
    /// its answer ready; once a run. Nothing without known folders.
    pub(crate) fn warm(&self) {
        drop(self.precheck_started());
    }

    /// The run's pre-check, started now when it was not yet; `true` with it when this call
    /// started it.
    fn precheck_started(&self) -> Option<(Arc<Pending<bool>>, bool)> {
        let known = Arc::clone(self.known.as_ref()?);
        let mut slot = lock(&self.precheck);
        if let Some(pending) = &*slot {
            return Some((Arc::clone(pending), false));
        }
        let probes = Arc::clone(&self.probes);
        let pending = Pending::start("clauseo-picker-precheck", true, move || {
            probes.would_stall(&known, PROBE_WAIT)
        });
        *slot = Some(Arc::clone(&pending));
        Some((pending, true))
    }

    /// Whether the native dialog would stall (`stall.rs`); never without known folders. The
    /// run's answer ([`Chooser::warm`]): at once when it has landed, else when it lands, at most
    /// `PROBE_WAIT` after it started. "Stalls" holds for the run: Windows' dialog stays slow for
    /// minutes. "Answers" may go stale (a share gone down since), so it is checked again, which
    /// takes milliseconds while the servers answer, and the new answer kept.
    pub(crate) fn would_stall(&self) -> bool {
        let Some((pending, fresh)) = self.precheck_started() else {
            return false;
        };
        if !fresh && pending.now() == Some(false) {
            let Some(known) = self.known.as_ref() else {
                return false;
            };
            let again = self.probes.would_stall(known, PROBE_WAIT);
            *lock(&self.precheck) = Some(Pending::landed(again));
            return again;
        }
        // The work ends within PROBE_WAIT of its start; past that and the hand-in, it stalls.
        pending.get(PROBE_WAIT + HAND_IN).unwrap_or(true)
    }

    /// Whether a session may open at `now`: fewer than [`SESSIONS_MAX`] this run and than
    /// [`SESSIONS_PER_MINUTE`] in the minute before `now`.
    pub(crate) fn may_open(&self, now: Instant) -> bool {
        room(&lock(&self.opened), now)
    }

    /// Opens a session, closing any other, unless [`Chooser::may_open`] says no (`None`: draw
    /// the native dialog). Waits on nothing: its drives and servers are read once, on a thread,
    /// at most `DRIVES_WAIT` (none past it), and [`Chooser::starts`] and [`Chooser::list`]
    /// wait for them. `refused` is marked in listings.
    pub(crate) fn open(
        &self,
        home: Option<&Path>,
        last_pick: Option<&Path>,
        refused: &[PathBuf],
        now: Instant,
    ) -> Option<ChooserStart> {
        {
            let mut opened = lock(&self.opened);
            if !room(&opened, now) {
                return None;
            }
            opened.push(now);
        }
        let (machine, outstanding) = (Arc::clone(&self.machine), Arc::clone(&self.outstanding));
        let drives = Pending::start("clauseo-chooser-drives", Drives::default(), move || {
            bounded(&outstanding, DRIVES_WAIT, move || machine.drives()).unwrap_or_default()
        });
        let id = u32::try_from(RandomState::new().hash_one(now) & u64::from(u32::MAX)).unwrap_or(1);
        let refused = refused
            .iter()
            .filter_map(|path| path.to_str().and_then(Place::from_os))
            .collect();
        *lock(&self.session) = Some(Session {
            id,
            drives,
            asked: (home.map(Path::to_owned), last_pick.map(Path::to_owned)),
            ready: None,
            refused,
            current: None,
            listing: Vec::new(),
            network_root: None,
            listings: 0,
            used: now,
        });
        Some(ChooserStart { chooser: id })
    }

    /// The places session `chooser` may start from, once its drives landed (at most
    /// `DRIVES_WAIT` after it opened): the local drives, and home, the Desktop and the last
    /// pick's folder each only when local. `command.invalid` for an unknown, closed or expired
    /// session.
    pub(crate) fn starts(&self, chooser: u32, now: Instant) -> Result<Vec<StartPoint>, HostError> {
        let ready = self.ready(chooser, now)?;
        Ok(ready
            .starts
            .iter()
            .zip(0_u8..)
            .map(|((kind, place), key)| StartPoint {
                key,
                kind: *kind,
                display: match kind {
                    StartKind::Drive => place.root_label(),
                    _ => shown(place, ready.home.as_ref()).0,
                },
            })
            .collect())
    }

    /// Session `chooser`'s drives and what they decide, waiting for the drives when they have
    /// not landed; the session's lock is not held meanwhile.
    fn ready(&self, chooser: u32, now: Instant) -> Result<Ready, HostError> {
        let drives = {
            let mut guard = lock(&self.session);
            let session = live(&mut guard, chooser, now)?;
            if let Some(ready) = &session.ready {
                return Ok(ready.clone());
            }
            Arc::clone(&session.drives)
        };
        // The read itself ends within DRIVES_WAIT; past that and the hand-in, no drives.
        let drives = drives.get(DRIVES_WAIT + HAND_IN).unwrap_or_default();
        let mut guard = lock(&self.session);
        let session = live(&mut guard, chooser, now)?;
        let asked = &session.asked;
        let ready = session
            .ready
            .get_or_insert_with(|| ready_from(drives, asked.0.as_deref(), asked.1.as_deref()));
        Ok(ready.clone())
    }

    /// Goes where `go` says in session `chooser` and lists the folder there; blocks at most
    /// `LIST_WAIT`, after the session's drives landed. `command.invalid` for an unknown, closed
    /// or expired session, an index past the last listing, a start key it did not give and a
    /// path [`Place::typed`] refuses; `transport.unauthorized` past [`LISTINGS_MAX`].
    pub(crate) fn list(&self, chooser: u32, go: Go, now: Instant) -> Result<Listing, HostError> {
        let ready = self.ready(chooser, now)?;
        let home = ready.home.clone();
        let (target, typed, network, here, refused) = {
            let mut guard = lock(&self.session);
            let session = live(&mut guard, chooser, now)?;
            if session.listings >= LISTINGS_MAX {
                return Err(HostError::new(
                    ErrorCode::TransportUnauthorized,
                    "this folder chooser has listed all it may",
                ));
            }
            session.listings += 1;
            let here = session.current.clone();
            let (target, typed) = match go {
                Go::Start { key } => (
                    ready
                        .starts
                        .get(usize::from(key))
                        .map(|(_, place)| place.clone()),
                    false,
                ),
                Go::Into { index } => {
                    let item = usize::try_from(index)
                        .ok()
                        .and_then(|i| session.listing.get(i));
                    match (item, &here) {
                        (Some(item), Some(here)) if item.refused => {
                            let listing = shown_as(here, home.as_ref(), ListState::NotAllowed);
                            return Ok(listing);
                        }
                        (Some(item), Some(here)) => (here.child(&item.name), false),
                        _ => (None, false),
                    }
                }
                Go::Up => (here.as_ref().map(Place::up), false),
                Go::Again => (here.clone(), false),
                Go::Typed { path } => (Place::typed(&path), true),
            };
            let target = target.ok_or_else(|| invalid("no such folder in this chooser"))?;
            let start = ready.starts.iter().any(|(_, start)| start.same(&target));
            let refused = session.refused.clone();
            if !start && refused.iter().any(|no| no.same(&target)) {
                return Ok(not_allowed(here.as_ref(), home.as_ref()));
            }
            // From the session's one read of the drives: nothing here waits on the network.
            let network = on_network(&ready.drives, &target);
            let reachable = if typed {
                target.share_allowed(&ready.drives.servers)
            } else {
                !network || under(session.network_root.as_ref(), &target)
            };
            if !reachable {
                return Ok(not_allowed(here.as_ref(), home.as_ref()));
            }
            (target, typed, network, here, refused)
        };
        let machine = Arc::clone(&self.machine);
        let path = target.clone();
        let read = bounded(&self.outstanding, LIST_WAIT, move || {
            // A typed path goes through no link, hidden or system folder, and through none whose
            // attributes cannot be read; only its last folder may be missing (`not_found`).
            let folders = path.below_root();
            let blocked = typed
                && folders.iter().enumerate().any(|(depth, folder)| {
                    match machine.entry(&folder.text()) {
                        Some(entry) => entry.link || entry.hidden,
                        None => depth + 1 < folders.len(),
                    }
                });
            (!blocked).then(|| machine.read(&path.text()))
        });
        let state = match read {
            None => ListState::NoAnswer,
            Some(None) => return Ok(not_allowed(here.as_ref(), home.as_ref())),
            Some(Some(Err(error))) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Ok(not_allowed(here.as_ref(), home.as_ref()));
            }
            Some(Some(Err(_))) => ListState::NotFound,
            Some(Some(Ok(entries))) => {
                let (names, more) = folders(entries);
                let items = names
                    .into_iter()
                    .map(|name| {
                        let refused = target
                            .child(&name)
                            .is_some_and(|child| refused.iter().any(|no| no.same(&child)));
                        Item { name, refused }
                    })
                    .collect();
                ListState::Folders { items, more }
            }
        };
        let mut guard = lock(&self.session);
        let session = live(&mut guard, chooser, now)?;
        if network && typed {
            session.network_root = Some(target.root());
        }
        session.listing = match &state {
            ListState::Folders { items, .. } => items.clone(),
            _ => Vec::new(),
        };
        session.current = Some(target.clone());
        Ok(shown_as(&target, home.as_ref(), state))
    }

    /// The folder session `chooser` shows, for `chooser_choose`.
    pub(crate) fn current(&self, chooser: u32, now: Instant) -> Result<String, HostError> {
        let mut guard = lock(&self.session);
        let session = live(&mut guard, chooser, now)?;
        session
            .current
            .as_ref()
            .map(Place::text)
            .ok_or_else(|| invalid("no folder is shown"))
    }

    /// Closes session `chooser`; `command.invalid` when it is not the live one.
    pub(crate) fn close(&self, chooser: u32) -> Result<(), HostError> {
        let mut guard = lock(&self.session);
        match &*guard {
            Some(session) if session.id == chooser => {
                *guard = None;
                Ok(())
            }
            _ => Err(invalid("no such folder chooser")),
        }
    }
}

/// What `drives` decide for a session: home when local, and the start points: the local drives,
/// then home, the Desktop and the folder above `last_pick`, each only when local.
fn ready_from(drives: Drives, home: Option<&Path>, last_pick: Option<&Path>) -> Ready {
    let local = |path: Option<&Path>| {
        path.and_then(Path::to_str)
            .and_then(Place::from_os)
            .filter(|place| !on_network(&drives, place))
    };
    let home = local(home);
    let mut starts: Vec<(StartKind, Place)> = drives
        .local
        .iter()
        .copied()
        .filter_map(Place::drive)
        .map(|drive| (StartKind::Drive, drive))
        .collect();
    let named = [
        (StartKind::Home, home.clone()),
        (
            StartKind::Desktop,
            local(drives.desktop.as_deref().map(Path::new)),
        ),
        // The folder above the last pick, as `dialogs::start_folder` opens the native dialog.
        (
            StartKind::LastPick,
            local(last_pick)
                .filter(|pick| !pick.parts().is_empty())
                .map(|pick| pick.up()),
        ),
    ];
    starts.extend(
        named
            .into_iter()
            .filter_map(|(kind, place)| Some((kind, place?))),
    );
    Ready {
        drives,
        starts,
        home,
    }
}

/// Whether a session may open at `now`, after the ones `opened`.
fn room(opened: &[Instant], now: Instant) -> bool {
    let last_minute = opened
        .iter()
        .filter(|at| now.saturating_duration_since(**at) < Duration::from_secs(60))
        .count();
    opened.len() < SESSIONS_MAX && last_minute < SESSIONS_PER_MINUTE
}

/// The live session `chooser`, which `now` marks used; an expired one is closed.
fn live(
    guard: &mut Option<Session>,
    chooser: u32,
    now: Instant,
) -> Result<&mut Session, HostError> {
    if guard
        .as_ref()
        .is_some_and(|s| now.saturating_duration_since(s.used) > IDLE_MAX)
    {
        *guard = None;
    }
    match guard {
        Some(session) if session.id == chooser => {
            session.used = now;
            Ok(session)
        }
        _ => Err(invalid("no such folder chooser")),
    }
}

/// Whether `place` is under the typed share `root`.
fn under(root: Option<&Place>, place: &Place) -> bool {
    root.is_some_and(|root| place.within(root))
}

/// The listing that refuses to go: the folder shown stays.
fn not_allowed(here: Option<&Place>, home: Option<&Place>) -> Listing {
    match here {
        Some(here) => shown_as(here, home, ListState::NotAllowed),
        None => Listing {
            display: String::new(),
            crumbs: Vec::new(),
            state: ListState::NotAllowed,
        },
    }
}

/// `place` shown, with `state`.
fn shown_as(place: &Place, home: Option<&Place>, state: ListState) -> Listing {
    let (display, crumbs) = shown(place, home);
    Listing {
        display,
        crumbs,
        state,
    }
}

/// How `place` shows, by `picked::display`'s rule in Windows' terms (names compared ignoring
/// case): under home, the parts below it joined with `\`; elsewhere the full path. Home itself
/// shows empty, so the page never sees the user's name; its crumb is empty too.
fn shown(place: &Place, home: Option<&Place>) -> (String, Vec<String>) {
    if let Some(home) = home.filter(|home| place.within(home)) {
        let below = &place.parts()[home.parts().len()..];
        let crumbs = std::iter::once(String::new())
            .chain(below.iter().cloned())
            .collect();
        return (below.join("\\"), crumbs);
    }
    let crumbs = std::iter::once(place.root_label())
        .chain(place.parts().iter().cloned())
        .collect();
    (place.text(), crumbs)
}

/// A `command.invalid` failure with `message`.
fn invalid(message: &str) -> HostError {
    HostError::new(ErrorCode::CommandInvalid, message)
}

/// The machine off Windows, where no session opens: nothing to list.
#[cfg(not(windows))]
struct NoMachine;

#[cfg(not(windows))]
impl Machine for NoMachine {
    fn drives(&self) -> Drives {
        Drives::default()
    }
    fn read(&self, _folder: &str) -> std::io::Result<Vec<Entry>> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn entry(&self, _path: &str) -> Option<Entry> {
        None
    }
}

#[cfg(not(windows))]
use listing::Entry;
