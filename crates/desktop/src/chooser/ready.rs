//! What a session reads of the machine as it opens, on its own thread, and the start points that
//! read leaves.

use std::path::{Path, PathBuf};

use super::listing::{Drives, on_network};
use super::place::Place;
use super::wire::StartKind;

/// What a session reads of the machine as it opens, on its thread: the drives, home, and the
/// folders marked refused in listings.
#[derive(Clone, Default)]
pub(super) struct Read {
    pub(super) drives: Drives,
    pub(super) home: Option<PathBuf>,
    pub(super) refused: Vec<Place>,
}

/// A session's read, and the start points and home it leaves.
#[derive(Clone)]
pub(super) struct Ready {
    pub(super) drives: Drives,
    pub(super) starts: Vec<(StartKind, Place)>,
    pub(super) home: Option<Place>,
    pub(super) refused: Vec<Place>,
}

/// What a session's `read` decides: home when local, and the start points: the local drives,
/// then home, the Desktop and the folder above `last_pick`, each only when local.
pub(super) fn ready_from(read: Read, last_pick: Option<&Path>) -> Ready {
    let Read {
        drives,
        home,
        refused,
    } = read;
    let home = home.as_deref();
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
        refused,
    }
}
