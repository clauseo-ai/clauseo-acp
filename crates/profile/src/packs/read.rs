//! The one read of a pack: a walk that never follows a link, checks every name and cap before
//! it reads, and reads each file through a handle that is checked again once open.
//!
//! The walk is the same on every OS; each OS supplies the folder handle it walks with
//! ([`super::unix`], [`super::windows`]):
//!
//! - **Unix** walks by file descriptor: each folder is opened with `O_DIRECTORY | O_NOFOLLOW`
//!   relative to its parent's descriptor and listed from it; each entry is `lstat`ed and opened
//!   relative to the same descriptor with `O_NOFOLLOW`, and the open descriptor must be the
//!   device and inode the `lstat` saw. No path is resolved twice, so a folder swapped for a link
//!   mid-walk reads nothing from outside.
//! - **Windows** examines each entry with `symlink_metadata` and refuses every reparse point
//!   (junctions, symlinks, cloud placeholders, which a read would fetch). Each file is opened
//!   with `FILE_FLAG_OPEN_REPARSE_POINT`, and its handle must show no reparse point, one link,
//!   and a final path equal to the pack's own plus the relative path.
//! - Every name must be UTF-8 and every path plain (`skills::is_safe_path`), within
//!   [`PATH_MAX`] bytes and [`DEPTH_MAX`] folders, and not equal to another in all but case. A
//!   pack has at most [`FOLDERS_MAX`] folders, counted as each is found, and a folder is opened
//!   only when the walk reaches it, so the walk holds at most one folder open per depth level. A
//!   file with more than one hard link is refused. Each file's size is checked before the read,
//!   and the read stops one byte past the cap, so a file that grows during the read is refused.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;

use super::{
    DEPTH_MAX, FILE_MAX, FILES_MAX, FOLDERS_MAX, MANIFEST_MAX, PACK_MANIFEST, PACK_MAX, PATH_MAX,
    Refusal, refuse,
};
use crate::skills::is_safe_path;

#[cfg(unix)]
use super::unix::Folder;
#[cfg(windows)]
use super::windows::Folder;

/// One file of a pack as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackFile {
    /// Its path relative to the pack, `/`-separated.
    pub path: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
    /// Whether any Unix exec bit is set (always false on Windows).
    pub exec: bool,
}

/// A pack as read: every file, and every folder's relative path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackRead {
    /// The files, in no set order.
    pub files: Vec<PackFile>,
    /// The folders, `/`-separated, in no set order.
    pub folders: Vec<String>,
    /// The most folders the read held open at once, the pack's own included: at most one per
    /// depth level plus the folder being listed, so never more than [`DEPTH_MAX`] + 2.
    pub open_peak: usize,
}

/// What an OS's folder handle says an entry is, without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// A folder.
    Folder,
    /// A plain file of `len` bytes, and whether it has more than one hard link (Windows says
    /// no here and checks the handle instead).
    File { len: u64, hard_linked: bool },
    /// A symlink or any other reparse point.
    Link,
    /// Anything else: a pipe, a socket, a device.
    Other,
}

/// Every file under `folder`, read once into memory, in no set order. Refuses the pack on the
/// first link, bad name, special file or cap passed. Each folder is counted against
/// [`FOLDERS_MAX`] when it is found and opened only when the walk reaches it, so the folders held
/// open at once are the ones on the path being walked.
pub fn read_pack(folder: &Path) -> Result<PackRead, Refusal> {
    let mut walk = Walk::default();
    let root = walk.hold(Folder::open_root(folder)?);
    walk.list(&root, "")?;
    drop(root);
    while let Some((parent, name, rel)) = walk.pending.pop() {
        let dir = walk.hold(parent.folder.open_folder(&name, &rel)?);
        // Its parent stays open only while a sibling still waits.
        drop(parent);
        walk.list(&dir, &rel)?;
    }
    walk.read.open_peak = walk.peak;
    Ok(walk.read)
}

/// The walk's state: what was read, the totals, and the folders found but not yet listed, each
/// with its parent, its name and its path.
#[derive(Default)]
struct Walk {
    read: PackRead,
    total: u64,
    seen: BTreeSet<String>,
    pending: Vec<(Rc<Held>, String, String)>,
    open: Rc<Cell<usize>>,
    peak: usize,
}

/// A folder the walk holds open, counted while it lives.
struct Held {
    folder: Folder,
    open: Rc<Cell<usize>>,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.open.set(self.open.get().saturating_sub(1));
    }
}

impl Walk {
    /// `folder`, counted as open until its last holder drops it.
    fn hold(&mut self, folder: Folder) -> Rc<Held> {
        self.open.set(self.open.get() + 1);
        self.peak = self.peak.max(self.open.get());
        Rc::new(Held {
            folder,
            open: Rc::clone(&self.open),
        })
    }

    /// Checks each entry of `dir` (at `prefix` inside the pack), reads its files, and queues its
    /// folders, counting them.
    fn list(&mut self, dir: &Rc<Held>, prefix: &str) -> Result<(), Refusal> {
        for name in dir.folder.names()? {
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if !is_safe_path(&rel)
                || rel.len() > PATH_MAX
                || rel.matches('/').count() > DEPTH_MAX
                || !self.seen.insert(rel.to_ascii_lowercase())
            {
                return Err(Refusal::BadPath);
            }
            match dir.folder.kind(&name)? {
                Kind::Link => return Err(Refusal::Link),
                Kind::Other => return Err(Refusal::NotAFile),
                Kind::Folder => {
                    if self.read.folders.len() >= FOLDERS_MAX {
                        return Err(Refusal::TooBig);
                    }
                    self.read.folders.push(rel.clone());
                    self.pending.push((Rc::clone(dir), name, rel));
                }
                Kind::File { len, hard_linked } => {
                    self.file(&dir.folder, &name, rel, len, hard_linked)?;
                }
            }
        }
        Ok(())
    }

    /// Reads the file `name` of `dir` (`rel` inside the pack), within the caps.
    fn file(
        &mut self,
        dir: &Folder,
        name: &str,
        rel: String,
        len: u64,
        hard_linked: bool,
    ) -> Result<(), Refusal> {
        let cap = if rel == PACK_MANIFEST {
            MANIFEST_MAX
        } else {
            FILE_MAX
        };
        if len > cap || self.read.files.len() >= FILES_MAX || self.total + len > PACK_MAX {
            return Err(Refusal::TooBig);
        }
        if hard_linked {
            return Err(Refusal::HardLink);
        }
        let (file, exec) = dir.open_file(name, &rel)?;
        let bytes = read_capped(file, len, cap)?;
        self.total += bytes.len() as u64;
        if self.total > PACK_MAX {
            return Err(Refusal::TooBig);
        }
        self.read.files.push(PackFile {
            path: rel,
            bytes,
            exec,
        });
        Ok(())
    }
}

/// `reader`'s bytes, refused when there are more than `cap`; `hint` sizes the buffer.
pub(crate) fn read_capped(reader: impl Read, hint: u64, cap: u64) -> Result<Vec<u8>, Refusal> {
    let mut bytes = Vec::with_capacity(usize::try_from(hint.min(cap)).unwrap_or(0));
    reader
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(refuse(Refusal::Io))?;
    if bytes.len() as u64 > cap {
        return Err(Refusal::TooBig);
    }
    Ok(bytes)
}

/// An error unless the pack's own folder is a folder and no link (`symlink_metadata`; on
/// Windows no reparse point either).
pub(super) fn check_root(folder: &Path) -> Result<(), Refusal> {
    let meta = std::fs::symlink_metadata(folder).map_err(refuse(Refusal::Missing))?;
    if meta.file_type().is_symlink() || is_reparse(&meta) {
        return Err(Refusal::Link);
    }
    if !meta.is_dir() {
        return Err(Refusal::Missing);
    }
    Ok(())
}

/// Whether `meta` carries `FILE_ATTRIBUTE_REPARSE_POINT`.
#[cfg(windows)]
pub(super) fn is_reparse(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    /// `FILE_ATTRIBUTE_REPARSE_POINT`.
    const REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & REPARSE_POINT != 0
}

/// No reparse points off Windows.
#[cfg(not(windows))]
pub(super) fn is_reparse(_meta: &std::fs::Metadata) -> bool {
    false
}

/// What every OS's folder handle offers the walk. Off Unix and Windows there is none, and
/// every pack is refused.
#[cfg(not(any(unix, windows)))]
struct Folder;

#[cfg(not(any(unix, windows)))]
impl Folder {
    fn open_root(_folder: &Path) -> Result<Self, Refusal> {
        Err(Refusal::Io)
    }
    fn names(&self) -> Result<Vec<String>, Refusal> {
        Err(Refusal::Io)
    }
    fn kind(&self, _name: &str) -> Result<Kind, Refusal> {
        Err(Refusal::Io)
    }
    fn open_folder(&self, _name: &str, _rel: &str) -> Result<Self, Refusal> {
        Err(Refusal::Io)
    }
    fn open_file(&self, _name: &str, _rel: &str) -> Result<(std::fs::File, bool), Refusal> {
        Err(Refusal::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader that yields more than the metadata said: the file grew during the read.
    struct Growing(usize);

    impl Read for Growing {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min(self.0);
            buf.iter_mut().take(n).for_each(|byte| *byte = b'x');
            self.0 -= n;
            Ok(n)
        }
    }

    #[test]
    fn a_file_growing_during_the_read_is_refused() {
        let cap = 1024;
        // The metadata said 10 bytes; the file now holds cap + 1.
        let grown = Growing(usize::try_from(cap).unwrap() + 1);
        assert_eq!(read_capped(grown, 10, cap), Err(Refusal::TooBig));
        let at_cap = Growing(usize::try_from(cap).unwrap());
        assert_eq!(read_capped(at_cap, 10, cap).unwrap().len(), 1024);
    }
}
