//! A journal in memory, for the tests on every OS: the roots' facts, the journal's state and
//! records, the folders' places and the trees' file IDs, each settable, with failures to give.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use super::journal::{
    ATTRIBUTE_DIRECTORY, Change, Flow, Found, Journal, JournalError, JournalState, Listed,
    RootFacts, Usn,
};

/// The fake's volume serial.
pub(crate) const VOLUME: u64 = 7;

/// The journal, in memory.
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeJournal {
    pub(crate) roots: BTreeMap<PathBuf, RootFacts>,
    pub(crate) journal: u64,
    /// The lowest readable USN.
    pub(crate) first: Usn,
    /// The next USN when no record is past it.
    pub(crate) next: Usn,
    pub(crate) records: Vec<Change>,
    pub(crate) places: BTreeMap<u64, Found>,
    pub(crate) trees: BTreeMap<PathBuf, Vec<Listed>>,
    pub(crate) query_fails: Option<JournalError>,
    pub(crate) read_fails: Option<JournalError>,
    pub(crate) tree_fails: bool,
    pub(crate) place_fails: bool,
    /// How many records the last read handed over.
    pub(crate) handed: std::cell::Cell<usize>,
}

impl FakeJournal {
    /// The USN after every record.
    pub(crate) fn end(&self) -> Usn {
        self.records
            .last()
            .map_or(self.next, |c| (c.usn + 1).max(self.next))
    }

    /// A record of `reason` about `file` in `parent`, `folder` or not, after every other.
    pub(crate) fn change(&mut self, file: u64, parent: u64, reason: u32, folder: bool) {
        let usn = self.end();
        self.records.push(Change {
            usn,
            file,
            parent,
            reason,
            attributes: if folder { ATTRIBUTE_DIRECTORY } else { 0x20 },
        });
        self.next = usn + 1;
    }
}

impl Journal for FakeJournal {
    fn root(&self, path: &Path) -> io::Result<RootFacts> {
        self.roots
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn query(&self, _root: &Path) -> Result<JournalState, JournalError> {
        if let Some(error) = self.query_fails {
            return Err(error);
        }
        Ok(JournalState {
            id: self.journal,
            first: self.first,
            next: self.end(),
        })
    }

    fn read(
        &self,
        _root: &Path,
        journal: u64,
        from: Usn,
        _to: Usn,
        each: &mut dyn FnMut(&Change) -> Flow,
    ) -> Result<(), JournalError> {
        if let Some(error) = self.read_fails {
            return Err(error);
        }
        if journal != self.journal || from < self.first {
            return Err(JournalError::Lost);
        }
        let mut handed = 0;
        for change in self.records.iter().filter(|c| c.usn >= from) {
            handed += 1;
            if each(change) == Flow::Stop {
                break;
            }
        }
        self.handed.set(handed);
        Ok(())
    }

    fn path_of(&self, _root: &Path, volume: u64, id: u64) -> io::Result<Option<Found>> {
        if volume != VOLUME || self.place_fails {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        Ok(self.places.get(&id).cloned())
    }

    fn tree(&self, root: &Path) -> io::Result<Vec<Listed>> {
        if self.tree_fails {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(self.trees.get(root).cloned().unwrap_or_default())
    }
}

/// `FakeJournal` for the own roots `roots` (path and file ID), each on [`VOLUME`], NTFS,
/// fixed, its final path its own; each root's tree `files` files directly in it and one
/// folder `a` (ID root + 1) holding one file (root + 2). File `i` of a root is `root * 1000 + i`.
pub(crate) fn journal_for(roots: &[(PathBuf, u64)], files: u64) -> FakeJournal {
    let mut journal = FakeJournal {
        journal: 0x1dd5,
        first: 10,
        next: 1_000,
        ..FakeJournal::default()
    };
    for (path, id) in roots {
        let facts = RootFacts {
            volume: VOLUME,
            id: Some(*id),
            ntfs: true,
            removable: false,
            path: path.clone(),
        };
        journal.roots.insert(path.clone(), facts);
        journal.places.insert(*id, found(path.clone()));
        journal.places.insert(id + 1, found(path.join("a")));
        let mut tree = vec![
            Listed {
                id: id + 1,
                parent: *id,
                folder: true,
            },
            Listed {
                id: id + 2,
                parent: id + 1,
                folder: false,
            },
        ];
        tree.extend((0..files).map(|i| Listed {
            id: id * 1000 + i,
            parent: *id,
            folder: false,
        }));
        journal.trees.insert(path.clone(), tree);
    }
    journal
}

/// A folder found at `path`, not a link.
pub(crate) fn found(path: PathBuf) -> Found {
    Found { path, link: false }
}
