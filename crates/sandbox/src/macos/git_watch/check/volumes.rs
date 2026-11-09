//! The volumes under a watched folder. A volume mounted inside a live matter brings a
//! ready-made tree with no event for what is in it, and one the watch never enters (a network
//! share, an automount) would never be scanned at all. So the launch walk notes every volume
//! it meets under the folder, and any other one a later scan meets is a find: while the agent
//! may run it ends the session; in the final scan it is kept as stuck until it is ejected.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::super::state::Volumes;
use crate::policy::path_key;

/// The launch's volumes, as a scan looks them up.
pub(super) struct Seen {
    devices: HashSet<u64>,
    /// Folders on volumes a scan never enters, by path key.
    refused: HashSet<PathBuf>,
}

impl Seen {
    /// Those of a launch's record.
    pub(super) fn of(volumes: &Volumes) -> Seen {
        Seen {
            devices: volumes.devices.iter().copied().collect(),
            refused: volumes
                .refused
                .iter()
                .map(|dir| path_key(&dir.path()))
                .collect(),
        }
    }

    /// Whether a folder on device `dev` is on a volume the launch didn't meet.
    pub(super) fn new_device(&self, dev: u64) -> bool {
        !self.devices.contains(&dev)
    }

    /// Whether `dir`, on a volume a scan never enters, wasn't one at launch.
    pub(super) fn new_refusal(&self, dir: &Path) -> bool {
        !self.refused.contains(&path_key(dir))
    }
}

/// The volumes a launch walk meets, for its record.
#[derive(Default)]
pub(super) struct Met {
    devices: HashSet<u64>,
    refused: Vec<PathBuf>,
}

impl Met {
    /// A folder on device `dev`.
    pub(super) fn device(&mut self, dev: u64) {
        self.devices.insert(dev);
    }

    /// A folder on a volume a scan never enters.
    pub(super) fn refused(&mut self, dir: &Path) {
        self.refused.push(dir.to_owned());
    }

    /// The record.
    pub(super) fn record(self) -> Volumes {
        let mut devices: Vec<u64> = self.devices.into_iter().collect();
        devices.sort_unstable();
        Volumes {
            devices,
            refused: self
                .refused
                .iter()
                .map(|dir| dir.as_path().into())
                .collect(),
        }
    }
}
