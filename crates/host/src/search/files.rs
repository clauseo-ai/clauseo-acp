//! A matter's file names at a checkpoint, for the index: listed on the search thread from the
//! matter's history (`Matter::files_at`), never on the actor.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use clauseo_matters::Matter;
use clauseo_protocol::{CheckpointId, MatterId};
use clauseo_store::{MatterRow, Store};
use futures::executor::block_on;

use crate::log_file::Cause;

/// Matters kept open for their listings.
const OPEN_CAP: usize = 8;

/// Every matter's folder, and the few matters kept open.
pub(super) struct Files {
    store: Store,
    settings: clauseo_matters::Settings,
    folders: BTreeMap<MatterId, PathBuf>,
    opened: VecDeque<Matter>,
}

impl Files {
    pub(super) fn new(store: Store, settings: clauseo_matters::Settings) -> Self {
        Self {
            store,
            settings,
            folders: BTreeMap::new(),
            opened: VecDeque::new(),
        }
    }

    /// Notes a matter row's folder.
    pub(super) fn note(&mut self, row: &MatterRow) {
        self.folders.insert(row.id, PathBuf::from(&row.folder_path));
    }

    /// Every matter whose row was noted.
    pub(super) fn matters(&self) -> impl Iterator<Item = MatterId> + '_ {
        self.folders.keys().copied()
    }

    /// Reads every matter's folder from the store again (one moved, or is new).
    pub(super) fn refresh(&mut self) {
        match block_on(self.store.matters()) {
            Ok(rows) => {
                for row in &rows {
                    self.note(row);
                }
            }
            Err(error) => tracing::warn!(error = %Cause(&error), "the matter rows were not read"),
        }
    }

    /// Closes the matter if it is open.
    pub(super) fn close(&mut self, matter: MatterId) {
        self.opened.retain(|open| open.id() != matter);
    }

    /// The matter's file names at `checkpoint`, or `None` (logged) when its history does not
    /// open or lacks the checkpoint.
    pub(super) fn list(
        &mut self,
        matter: MatterId,
        checkpoint: CheckpointId,
    ) -> Option<Vec<String>> {
        if let Some(at) = self.opened.iter().position(|open| open.id() == matter) {
            if let Some(Ok(files)) = self.opened.get(at).map(|open| open.files_at(checkpoint)) {
                return Some(paths(files));
            }
            // A checkpoint newer than the open timeline knows, or a moved folder: reopen.
            self.opened.remove(at);
        }
        let mut failed = None;
        for attempt in 0..2 {
            if attempt == 1 || !self.folders.contains_key(&matter) {
                self.refresh();
            }
            let Some(folder) = self.folders.get(&matter).cloned() else {
                continue;
            };
            let listed = Matter::open(self.settings.clone(), matter, &folder).and_then(|open| {
                let files = open.files_at(checkpoint)?;
                Ok((open, files))
            });
            match listed {
                Ok((open, files)) => {
                    if self.opened.len() >= OPEN_CAP {
                        self.opened.pop_front();
                    }
                    self.opened.push_back(open);
                    return Some(paths(files));
                }
                Err(error) => failed = Some(error),
            }
        }
        if let Some(error) = failed {
            tracing::warn!(
                %matter,
                error = %Cause(&error),
                "a matter's file names were not listed"
            );
        } else {
            tracing::warn!(%matter, "a matter with no row has no file names");
        }
        None
    }
}

fn paths(files: Vec<clauseo_matters::VersionFile>) -> Vec<String> {
    files
        .into_iter()
        .map(|file| file.path.to_string())
        .collect()
}
