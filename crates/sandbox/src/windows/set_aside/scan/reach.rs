//! The live watch's reads by key (`live.rs`, rule 8): a path below the watched folder reached
//! through handles alone. The watched folder is opened as itself ([`Volume::open_root`]), then
//! each folder on the way relative to the one before, as itself ([`Volume::open_folder`]), and
//! the item last ([`Volume::look`]). A link on the way is never followed: the read stops there
//! ([`Missed::Link`]), so a symbolic link to a network share partway along a reported path never
//! makes the lawyer's process sign in to that share, as a read by the full path would.

use std::ffi::OsString;
use std::io;
use std::path::Path;

use super::super::files::FileId;
use super::super::run::Volume;
use super::Kind;

/// What is at a name: its file ID and what it is; `None` when nothing is there.
pub(in crate::windows::set_aside) type Seen = Option<(FileId, Kind)>;

/// Why a walk by key didn't reach its folder.
#[derive(Debug)]
pub(in crate::windows::set_aside) enum Missed {
    /// A folder on the way is missing, or is a file: nothing is at the key.
    Gone,
    /// A folder on the way is a link: what is past it isn't in the watched folder.
    Link,
    /// The watched folder or a folder on the way couldn't be opened.
    Failed(io::Error),
}

/// The folder at `key` below `root` (`root` itself for `[]`), each part opened relative to the
/// one before as itself.
pub(in crate::windows::set_aside) fn open_key<V: Volume>(
    volume: &V,
    root: &Path,
    key: &[OsString],
) -> Result<V::Folder, Missed> {
    let mut at = volume.open_root(root).map_err(Missed::Failed)?;
    for part in key {
        at = match volume.open_folder(&at, part) {
            Ok(inner) => inner,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(Missed::Gone),
            // Not opened as a folder: a link or a file there now, or one that can't be read.
            Err(error) => {
                return Err(match volume.look(&at, part) {
                    Ok(Some((_, Kind::Link))) => Missed::Link,
                    Ok(Some((_, Kind::Other)) | None) => Missed::Gone,
                    Ok(Some((_, Kind::Folder))) | Err(_) => Missed::Failed(error),
                });
            }
        };
    }
    Ok(at)
}

/// The folder the item at `key` below `root` is in, opened as [`open_key`] opens it, and what
/// is at the item's own name, opened relative to it as itself (`None`: nothing). The item's own
/// read failing is [`Missed::Failed`] too.
pub(in crate::windows::set_aside) fn reach<V: Volume>(
    volume: &V,
    root: &Path,
    key: &[OsString],
) -> Result<(V::Folder, Seen), Missed> {
    let Some((name, parents)) = key.split_last() else {
        let error = io::Error::new(io::ErrorKind::InvalidInput, "the watched folder itself");
        return Err(Missed::Failed(error));
    };
    let folder = open_key(volume, root, parents)?;
    let found = volume.look(&folder, name).map_err(Missed::Failed)?;
    Ok((folder, found))
}
