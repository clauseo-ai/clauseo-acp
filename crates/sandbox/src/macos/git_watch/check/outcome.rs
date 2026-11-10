//! What a scan's finds come to: while the agent may run, any of them ends the session; once it
//! has ended, items are set aside and what can't be checked is kept as stuck.

use std::path::PathBuf;

use super::super::aside::Stuck;
use super::super::{Asks, Fs, Phase};
use super::{Item, Watch};

/// What a scan found: items to set aside, folders it could not open or list, each with the
/// error, and folders on volumes the launch didn't meet.
#[derive(Default)]
pub(super) struct Found {
    pub(super) items: Vec<Item>,
    pub(super) unreadable: Vec<(PathBuf, String)>,
    pub(super) mounts: Vec<PathBuf>,
}

impl Found {
    pub(super) fn any(&self) -> bool {
        !self.items.is_empty() || !self.unreadable.is_empty() || !self.mounts.is_empty()
    }
}

/// While the agent may run, any item ends the session, gone or not, as does a folder that
/// can't be read (it could hide one) or a volume the launch didn't meet; once it has ended,
/// each item still there, on a local volume, is set aside, and each folder that can't be read
/// and each such volume is kept as stuck: the matter's next launch is refused until it can be
/// read, or the volume is ejected.
pub(super) fn put<F: Fs>(watch: &mut Watch<F>, found: Found, phase: Phase) -> Asks {
    let mut asks = Asks::default();
    for path in found.mounts {
        match phase {
            Phase::Running => {
                tracing::warn!(
                    path = %path.display(),
                    "a volume appeared in the matter while the agent may run; ending the session"
                );
                asks.end = true;
            }
            Phase::Ended => watch.aside.stuck.push(Stuck {
                path,
                reason: "a disk or share was mounted there during the session".to_owned(),
                unreadable: false,
                mount: true,
            }),
        }
    }
    for (path, error) in found.unreadable {
        match phase {
            Phase::Running => {
                tracing::warn!(
                    path = %path.display(), %error,
                    "the git watch can't read a folder in the matter; ending the session"
                );
                asks.end = true;
            }
            Phase::Ended => watch.aside.stuck.push(Stuck {
                path,
                reason: format!("Clauseo can't read this folder: {error}"),
                unreadable: true,
                mount: false,
            }),
        }
    }
    for (path, what, kind) in found.items {
        match phase {
            Phase::Running => {
                tracing::warn!(
                    path = %path.display(), ?what,
                    "the git watch found a git item in the matter; ending the session"
                );
                asks.end = true;
            }
            Phase::Ended => {
                let local = path.parent().is_some_and(|dir| watch.fs.local(dir));
                if local && watch.fs.meta(&path).is_some() {
                    watch.aside.put(&watch.fs, &path, what, kind);
                }
            }
        }
    }
    asks
}
