//! The order an own root's facts are read in, apart from the Windows calls so it is tested on
//! every OS: the file system first, and the NTFS-only queries (the reparse tag, the file ID) only
//! on NTFS. FAT32 refuses both with `ERROR_INVALID_PARAMETER`; asked first, they made a FAT
//! root read as unreadable rather than as not NTFS (S15's VM runs 5 and 6).

use std::io;
use std::path::{Path, PathBuf};

use super::journal::RootFacts;
use crate::windows::writer::ApplyFailure;

/// The calls on an own root opened as itself.
pub(crate) trait RootProbe {
    /// Whether its volume's file system is NTFS.
    fn ntfs(&self) -> io::Result<bool>;
    /// Whether it is a junction, symbolic link or other name surrogate (NTFS only).
    fn surrogate(&self) -> io::Result<bool>;
    /// Its volume's serial and its 64-bit file reference, `None` when its file ID doesn't fit
    /// (NTFS only).
    fn id(&self) -> io::Result<(u64, Option<u64>)>;
    /// Its normalized final path.
    fn final_path(&self) -> io::Result<PathBuf>;
    /// Whether the volume at `path` is on a removable drive.
    fn removable(&self, path: &Path) -> bool;
}

/// The root's facts through `probe`. A volume that isn't NTFS comes back at once as that, with
/// no ID and counted removable (the launch walks in full either way); a name surrogate is
/// [`ApplyFailure::Changed`].
pub(crate) fn read_facts(probe: &impl RootProbe) -> io::Result<RootFacts> {
    if !probe.ntfs()? {
        return Ok(RootFacts {
            volume: 0,
            id: None,
            ntfs: false,
            removable: true,
            path: PathBuf::new(),
        });
    }
    if probe.surrogate()? {
        return Err(ApplyFailure::Changed.into());
    }
    let (volume, id) = probe.id()?;
    let path = probe.final_path()?;
    let removable = probe.removable(&path);
    Ok(RootFacts {
        volume,
        id,
        ntfs: true,
        removable,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root on NTFS or, with `fat`, on FAT32, which refuses the NTFS-only queries.
    struct Probe {
        fat: bool,
        surrogate: bool,
    }

    /// FAT32's answer to the reparse-tag and file-ID queries: `ERROR_INVALID_PARAMETER`.
    fn refused() -> io::Error {
        io::Error::from_raw_os_error(87)
    }

    impl RootProbe for Probe {
        fn ntfs(&self) -> io::Result<bool> {
            Ok(!self.fat)
        }
        fn surrogate(&self) -> io::Result<bool> {
            if self.fat {
                Err(refused())
            } else {
                Ok(self.surrogate)
            }
        }
        fn id(&self) -> io::Result<(u64, Option<u64>)> {
            if self.fat {
                Err(refused())
            } else {
                Ok((7, Some(100)))
            }
        }
        fn final_path(&self) -> io::Result<PathBuf> {
            Ok(PathBuf::from("/matter"))
        }
        fn removable(&self, _path: &Path) -> bool {
            false
        }
    }

    #[test]
    fn a_fat_root_is_not_ntfs_before_any_query_fat_refuses() {
        let facts = read_facts(&Probe {
            fat: true,
            surrogate: false,
        })
        .unwrap();
        assert!(!facts.ntfs);
        assert_eq!(facts.id, None);
    }

    #[test]
    fn an_ntfs_root_reads_whole_and_a_surrogate_one_is_refused() {
        let probe = Probe {
            fat: false,
            surrogate: false,
        };
        let facts = read_facts(&probe).unwrap();
        assert!(facts.ntfs && !facts.removable);
        assert_eq!((facts.volume, facts.id), (7, Some(100)));
        let link = Probe {
            fat: false,
            surrogate: true,
        };
        let error = read_facts(&link).unwrap_err();
        assert_eq!(ApplyFailure::of(&error), ApplyFailure::Changed);
    }
}
