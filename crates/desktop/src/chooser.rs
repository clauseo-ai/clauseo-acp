//! The in-app folder chooser, for a Windows machine whose own folder dialog would stall on a
//! share that is down (`stall.rs`). `pick_folder_or_choose` opens a session ([`Chooser::open`]);
//! the page then moves through folders the desktop listed, by index only, or to a path the lawyer
//! typed ([`Chooser::list`]), and `chooser_choose` ends in a native confirm before the folder
//! goes through the bridge's own `pick_folder` (`choose.rs`), so admission is unchanged.
//!
//! Limits: one session at a time (a new one closes the old), at most [`LISTINGS_MAX`] listings
//! in it, closed after [`IDLE_MAX`] idle, on choose and on cancel. Every read runs on a thread of
//! its own under `listing::LIST_WAIT`. Network folders are reachable only under a share the lawyer
//! typed; the start points are the fixed and removable drives, home, the Desktop and the last
//! pick's folder, never Documents, the Libraries or a network place.

mod choose;
mod listing;
mod place;
mod stall;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use std::hash::{BuildHasher as _, RandomState};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clauseo_protocol::{ErrorCode, HostError};
use serde::{Deserialize, Serialize};

pub(crate) use choose::{NativeFolderConfirm, choose_folder};
use listing::{LIST_WAIT, Machine, bounded, folders, on_network};
use place::Place;
use stall::{KnownFolders, PROBE_WAIT, Probes};
pub(crate) use stall::{PICKER_VAR, Route, route};

use crate::bridge::lock;
use crate::picked::PickedFolder;

/// The most listings one session gives.
pub(crate) const LISTINGS_MAX: u32 = 300;

/// How long a session lasts with no call.
pub(crate) const IDLE_MAX: Duration = Duration::from_secs(600);

/// What `pick_folder_or_choose` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PickAnswer {
    /// The native dialog picked a folder.
    Picked { folder: PickedFolder },
    /// The native dialog was cancelled.
    Cancelled,
    /// The native dialog would stall: choose in the page, starting from these.
    Choose { start: ChooserStart },
}

/// A chooser session and where it may start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct ChooserStart {
    /// The session, for `chooser_list`, `chooser_choose` and `chooser_cancel`.
    pub(crate) chooser: u32,
    /// The places to start from.
    pub(crate) starts: Vec<StartPoint>,
}

/// A place to start from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct StartPoint {
    /// What `Go::Start` names it by.
    pub(crate) key: u8,
    /// What it is.
    pub(crate) kind: StartKind,
    /// How it shows: `C:` for a drive, the `picked::display` rule otherwise, and empty for home.
    pub(crate) display: String,
}

/// What a start point is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartKind {
    /// A fixed or removable drive.
    Drive,
    /// The home folder.
    Home,
    /// The Desktop.
    Desktop,
    /// The folder of the last pick.
    LastPick,
}

/// Where `chooser_list` goes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Go {
    /// A start point, by its key.
    Start { key: u8 },
    /// A folder of the last listing, by its index.
    Into { index: u32 },
    /// The folder above, stopping at a drive or share root.
    Up,
    /// A path the lawyer typed (`place.rs` says which it takes).
    Typed { path: String },
    /// The folder shown, read again.
    Again,
}

/// A folder as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct Listing {
    /// The folder shown, by the `picked::display` rule; empty for home.
    pub(crate) display: String,
    /// Its parts from the root (`C:`, `\\server\share`), or from home (shown as an empty one).
    pub(crate) crumbs: Vec<String>,
    /// What reading it gave.
    pub(crate) state: ListState,
}

/// What reading a folder gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ListState {
    /// Its folders, and whether there were more than shown.
    Folders { items: Vec<Item>, more: bool },
    /// It did not answer in time.
    NoAnswer,
    /// It is not there.
    NotFound,
    /// The chooser does not open it; the folder shown stays.
    NotAllowed,
}

/// A folder in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct Item {
    /// Its name.
    pub(crate) name: String,
    /// Whether Clauseo refuses it (`refused_folders`); it does not open.
    pub(crate) refused: bool,
}

/// The chooser's state, managed by the app: the one session, and what it asks the machine.
#[derive(Clone)]
pub(crate) struct Chooser {
    machine: Arc<dyn Machine>,
    known: Option<Arc<dyn KnownFolders>>,
    probes: Arc<Probes>,
    session: Arc<Mutex<Option<Session>>>,
    outstanding: Arc<AtomicUsize>,
}

/// The live session.
struct Session {
    id: u32,
    starts: Vec<Place>,
    home: Option<Place>,
    refused: Vec<Place>,
    current: Option<Place>,
    listing: Vec<Item>,
    network_root: Option<Place>,
    listings: u32,
    used: Instant,
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

    /// A chooser asking `machine`, and `known` for the pre-check.
    fn with(machine: Arc<dyn Machine>, known: Option<Arc<dyn KnownFolders>>) -> Self {
        Self {
            machine,
            known,
            probes: Arc::default(),
            session: Arc::default(),
            outstanding: Arc::default(),
        }
    }

    /// Whether the native dialog would stall (`stall.rs`); never without known folders.
    pub(crate) fn would_stall(&self) -> bool {
        self.known
            .as_ref()
            .is_some_and(|known| self.probes.would_stall(known, PROBE_WAIT))
    }

    /// Opens a session, closing any other: its start points, of `home`, the Desktop and
    /// `last_pick`'s folder each only when local. `refused` is marked in listings.
    pub(crate) fn open(
        &self,
        home: Option<&Path>,
        last_pick: Option<&Path>,
        refused: &[PathBuf],
        now: Instant,
    ) -> ChooserStart {
        let machine = self.machine.as_ref();
        let local = |path: Option<&Path>| {
            path.and_then(Path::to_str)
                .and_then(Place::from_os)
                .filter(|place| !on_network(machine, place))
        };
        let home_place = local(home);
        let mut starts: Vec<(StartKind, Place)> = machine
            .drives()
            .into_iter()
            .filter_map(Place::drive)
            .map(|drive| (StartKind::Drive, drive))
            .collect();
        let desktop = machine.desktop();
        let named = [
            (StartKind::Home, home_place.clone()),
            (StartKind::Desktop, local(desktop.as_deref().map(Path::new))),
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
        let id = u32::try_from(RandomState::new().hash_one(now) & u64::from(u32::MAX)).unwrap_or(1);
        let points = starts
            .iter()
            .zip(0_u8..)
            .map(|((kind, place), key)| StartPoint {
                key,
                kind: *kind,
                display: match kind {
                    StartKind::Drive => place.root_label(),
                    _ => shown(place, home_place.as_ref()).0,
                },
            })
            .collect();
        let refused = refused
            .iter()
            .filter_map(|path| path.to_str().and_then(Place::from_os))
            .collect();
        *lock(&self.session) = Some(Session {
            id,
            starts: starts.into_iter().map(|(_, place)| place).collect(),
            home: home_place,
            refused,
            current: None,
            listing: Vec::new(),
            network_root: None,
            listings: 0,
            used: now,
        });
        ChooserStart {
            chooser: id,
            starts: points,
        }
    }

    /// Goes where `go` says in session `chooser` and lists the folder there; blocks at most
    /// `LIST_WAIT`. `command.invalid` for an unknown, closed or expired session, an index past
    /// the last listing, a start key it did not give and a path [`Place::typed`] refuses;
    /// `transport.unauthorized` past [`LISTINGS_MAX`].
    pub(crate) fn list(&self, chooser: u32, go: Go, now: Instant) -> Result<Listing, HostError> {
        let (target, typed, here, home, refused) = {
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
                Go::Start { key } => (session.starts.get(usize::from(key)).cloned(), false),
                Go::Into { index } => {
                    let item = usize::try_from(index)
                        .ok()
                        .and_then(|i| session.listing.get(i));
                    match (item, &here) {
                        (Some(item), Some(here)) if item.refused => {
                            let listing =
                                shown_as(here, session.home.as_ref(), ListState::NotAllowed);
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
            let start = session.starts.iter().any(|start| start.same(&target));
            let refused = session.refused.clone();
            if !start && refused.iter().any(|no| no.same(&target)) {
                return Ok(not_allowed(here.as_ref(), session.home.as_ref()));
            }
            if !typed && target.is_share() && !under(session.network_root.as_ref(), &target) {
                return Ok(not_allowed(here.as_ref(), session.home.as_ref()));
            }
            (target, typed, here, session.home.clone(), refused)
        };
        // Off the lock: a network drive's question may block.
        let network = on_network(self.machine.as_ref(), &target);
        let roots = lock(&self.session)
            .as_ref()
            .and_then(|s| s.network_root.clone());
        if network && !typed && !under(roots.as_ref(), &target) {
            return Ok(not_allowed(here.as_ref(), home.as_ref()));
        }
        let machine = Arc::clone(&self.machine);
        let text = target.text();
        let read = bounded(&self.outstanding, LIST_WAIT, move || machine.read(&text));
        let state = match read {
            None => ListState::NoAnswer,
            Some(Err(error)) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Ok(not_allowed(here.as_ref(), home.as_ref()));
            }
            Some(Err(_)) => ListState::NotFound,
            Some(Ok(entries)) => {
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
        Ok(shown_as(&target, session.home.as_ref(), state))
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
    fn drives(&self) -> Vec<char> {
        Vec::new()
    }
    fn remote_drive(&self, _letter: char) -> bool {
        false
    }
    fn desktop(&self) -> Option<String> {
        None
    }
    fn read(&self, _folder: &str) -> std::io::Result<Vec<Entry>> {
        Err(std::io::ErrorKind::Unsupported.into())
    }
    fn plain_dir(&self, _folder: &str) -> bool {
        false
    }
}

#[cfg(not(windows))]
use listing::Entry;
