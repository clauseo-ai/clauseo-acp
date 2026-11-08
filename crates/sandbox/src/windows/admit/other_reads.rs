//! The gate's listing for the reads of other matters' folders (R18, decision 0049): the entries
//! that carry each [`OtherMatterRead`] so that none of
//! [`REFUSED_IN_OTHER_MATTER`](crate::REFUSED_IN_OTHER_MATTER) is readable at any depth, with
//! no deny entry and no walk into a refused name.
//!
//! Each folder of a granted tree, at any depth, takes an allow for itself alone (listing it,
//! reading its attributes) and one for the files directly in it (`Inherit::FilesInFolder`:
//! object inherit, inherit only, no propagation), and each folder in it is taken the same way. No
//! allow reaches a folder by inheritance, so a folder made during the run (a `git init`'s `.git`,
//! a `.claude`) inherits nothing and stays closed behind the fence until the next launch lists
//! it, while a file made in an allowed folder reads at once (R18, MED 2). A folder with a refused
//! name among its files (a worktree's `.git` file) takes an allow on each of its other files
//! instead of the one for its files. A refused name gets nothing. So an allow reaches a refused
//! name neither on it nor by inheritance, and nothing is written in it or walked below it. Left
//! out as well: a junction or symbolic link (the walk never follows one, and an allow on one
//! would refuse the launch), and a file with another name elsewhere (an allow on it is refused,
//! as at every grant, and the walk skips it). A granted file takes an allow for itself alone,
//! unless it is one of those. The other matter's whole folder takes an allow on each of its
//! files too: the fence's deny for what is below it would beat an allow for its files, which
//! sits on the same folder and comes first (`fence.rs`).

#[cfg(any(windows, test, feature = "test-support"))]
use std::io;
use std::path::{Path, PathBuf};

#[cfg(any(windows, test, feature = "test-support"))]
use super::{FolderRefusal, FolderSupport, PathKind, refusal, refused};
use crate::windows::acl_plan::Inherit;
#[cfg(any(windows, test, feature = "test-support"))]
use crate::windows::acl_plan::in_folder;
#[cfg(any(windows, test, feature = "test-support"))]
use crate::{Error, OtherMatterRead, Policy};

/// One entry a read of another matter needs: `ReadAndExecute` for the pool user on
/// [`ReadGrant::path`], for the path alone or for the files directly in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadGrant {
    path: PathBuf,
    inherit: Inherit,
}

impl ReadGrant {
    /// The file or folder the allow is on.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// [`Inherit::ThisOnly`] or [`Inherit::FilesInFolder`].
    pub fn inherit(&self) -> Inherit {
        self.inherit
    }

    #[cfg(any(windows, test, feature = "test-support"))]
    fn this_only(path: PathBuf) -> ReadGrant {
        ReadGrant {
            path,
            inherit: Inherit::ThisOnly,
        }
    }

    #[cfg(any(windows, test, feature = "test-support"))]
    fn files_in(path: PathBuf) -> ReadGrant {
        ReadGrant {
            path,
            inherit: Inherit::FilesInFolder,
        }
    }
}

#[cfg(any(windows, test, feature = "test-support"))]
/// The entries of each of `policy`'s reads of other matters, in order, each once. Each granted
/// folder (a granted file's folder) must answer [`FolderSupport::Ok`] to `check`; any other
/// answer refuses with [`PathKind::OtherMatterRead`], and so does a listing that fails.
pub(super) fn reads(
    policy: &Policy,
    check: &mut impl FnMut(&Path) -> io::Result<FolderSupport>,
) -> Result<Vec<ReadGrant>, Error> {
    let mut out: Vec<ReadGrant> = Vec::new();
    for read in policy.other_matter_reads() {
        let folder = if read.is_file() {
            read.path().parent().unwrap_or(read.path())
        } else {
            read.path()
        };
        if let Some(why) = refusal(check(folder)) {
            return Err(refused(PathKind::OtherMatterRead, why));
        }
        let failed = |e: io::Error| {
            let why = FolderRefusal::CheckFailed(e.kind());
            refused(PathKind::OtherMatterRead, why)
        };
        let grants = shape(read).map_err(failed)?;
        for grant in grants {
            if !out.contains(&grant) {
                out.push(grant);
            }
        }
    }
    Ok(out)
}

#[cfg(any(windows, test, feature = "test-support"))]
/// The entries one read needs: see the module's docs.
fn shape(read: &OtherMatterRead) -> io::Result<Vec<ReadGrant>> {
    let path = read.path().to_owned();
    if read.is_file() {
        let skip = linked(&path)? || is_link(&path)?;
        return Ok(if skip {
            Vec::new()
        } else {
            vec![ReadGrant::this_only(path)]
        });
    }
    // The other matter's whole folder takes an allow on each file: one for its files would lose,
    // on every file, to the fence's deny for what is below the folder, which sits on the same
    // folder and comes first (R18).
    let whole = in_folder(&path, read.root()) && in_folder(read.root(), &path);
    folder(read, &path, whole)
}

/// The entries for `folder` and every folder in it, in name order, so a plan is the same from
/// one launch to the next: an allow on the folder alone, one for its files, or (`each`, or a
/// refused name among them) one on each file, then the entries of each folder in it.
#[cfg(any(windows, test, feature = "test-support"))]
fn folder(read: &OtherMatterRead, folder: &Path, each: bool) -> io::Result<Vec<ReadGrant>> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        // Not followed: a junction or symbolic link is one (on Windows a name surrogate).
        let kind = entry.file_type()?;
        children.push((entry.path(), kind.is_symlink(), kind.is_dir()));
    }
    children.sort();
    let mut refused_file = false;
    let mut grants = vec![ReadGrant::this_only(folder.to_owned())];
    let mut files = Vec::new();
    let mut inner = Vec::new();
    for (path, link, dir) in children {
        if read.refuses(&path) {
            refused_file |= !dir;
        } else if link {
            // Left out: see the module's docs.
        } else if dir {
            inner.extend(self::folder(read, &path, false)?);
        } else {
            files.push(path);
        }
    }
    if !each && !refused_file {
        grants.push(ReadGrant::files_in(folder.to_owned()));
    } else {
        for file in files {
            if !linked(&file)? {
                grants.push(ReadGrant::this_only(file));
            }
        }
    }
    grants.extend(inner);
    Ok(grants)
}

#[cfg(any(windows, test, feature = "test-support"))]
/// Whether `path` is a junction or symbolic link (on Windows any reparse point).
fn is_link(path: &Path) -> io::Result<bool> {
    super::is_reparse_point(path)
}

/// Whether the file at `path` has another name elsewhere (a hard link): an allow on it is
/// refused (`LinkedFile`), so it is left out.
#[cfg(windows)]
fn linked(path: &Path) -> io::Result<bool> {
    let object = crate::windows::acl::object::Object::open(path)?;
    Ok(!object.folder && object.links > 1)
}

/// Whether the file at `path` has another name elsewhere (a hard link): the tests' stand-in.
#[cfg(all(not(windows), any(test, feature = "test-support")))]
fn linked(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt as _;
    let meta = std::fs::symlink_metadata(path)?;
    Ok(meta.is_file() && meta.nlink() > 1)
}
