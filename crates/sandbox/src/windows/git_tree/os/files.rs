//! The Git trees' files and folders on NTFS. Every path is opened as itself
//! (`FILE_FLAG_OPEN_REPARSE_POINT`), a walk's and a chain's folders each relative to the one
//! above (`acl::object::Object`, `OBJ_DONT_REPARSE`), and every per-file call goes through the
//! `\\?\` form, so a deep path in a tree of ten thousand files is never cut at `MAX_PATH`.
#![expect(
    unsafe_code,
    reason = "FFI to CreateFileW, GetFileInformationByHandleEx and the per-file calls"
)]

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read as _};
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Component, Path, PathBuf};

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::Storage::FileSystem::{
    COPY_FILE_FAIL_IF_EXISTS, CopyFileExW, CreateDirectoryW, CreateFileW, CreateHardLinkW,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
    FILE_INFO_BY_HANDLE_CLASS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileAttributeTagInfo, FileBasicInfo, FileIdInfo,
    FileStandardInfo, GetFileInformationByHandleEx, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    OPEN_EXISTING,
};
use windows::core::PCWSTR;

use super::super::{Entry, FileId, Refused};
use crate::windows::FolderSupport;
use crate::windows::acl::object::{Handle, Object, os_error, reparse_refused};
use crate::windows::admin_copy::take_back::remove_taking_back;
use crate::windows::admin_copy::take_back_os::NtfsLockDown;
use crate::windows::admin_copy::{Found, level_safe, os as admin_os, root_safe};
use crate::windows::folder::{folder_support, verbatim_wide};

/// Whether `path` is a network path (`folder::folder_support`'s first and third steps): UNC,
/// on a remote drive, or a folder whose final path is UNC. A file is asked of its folder.
pub(super) fn is_network(path: &Path) -> Result<bool, String> {
    let folder = match admin_os::found(path) {
        Ok(Found::File) => path.parent().unwrap_or(path),
        Ok(Found::Folder | Found::Link | Found::Missing) | Err(_) => path,
    };
    match folder_support(folder) {
        Ok(support) => Ok(support == FolderSupport::Network),
        Err(error) => Err(format!(
            "asking whether {} is local: {error}",
            path.display()
        )),
    }
}

/// Opens each folder of `dir` from its volume's root, each relative to the one above, never
/// following a reparse point and refusing one met at any level, `dir` included.
pub(super) fn plain_chain(dir: &Path) -> Result<(), Refused> {
    chain(dir, |_, _| Ok(()))
}

/// Whether `dir` and every folder above it pass `admin_copy::check`'s rule 2, as
/// `check_chain` applies it: the volume root's owner and DACL [`root_safe`], every other
/// level's [`level_safe`], each read through the handle [`chain`] opened it as itself. A
/// reparse point or a file on the way is `false`; a level that can't be read fails.
pub(super) fn admin_only(dir: &Path) -> Result<bool, String> {
    let mut safe = true;
    let walked = chain(dir, |folder, depth| {
        let bytes = folder
            .security((OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0)
            .map_err(|error| other(dir, &format!("reading a level's security: {error}")))?;
        let rule = if depth == 0 { root_safe } else { level_safe };
        safe &= rule(&bytes);
        Ok(())
    });
    match walked {
        Ok(()) => Ok(safe),
        Err(Refused::Reparse) => Ok(false),
        Err(Refused::Other(why)) => Err(why),
    }
}

/// Opens each folder of `dir` from its volume's root (depth 0) down, each relative to the one
/// above, and hands each to `visit` with its depth: a reparse point at any level is
/// [`Refused::Reparse`], a file or a missing folder `Other`.
fn chain(
    dir: &Path,
    mut visit: impl FnMut(&Object, usize) -> Result<(), Refused>,
) -> Result<(), Refused> {
    let mut root = PathBuf::new();
    let mut names = Vec::new();
    for part in dir.components() {
        match part {
            Component::Prefix(_) | Component::RootDir if names.is_empty() => root.push(part),
            Component::Normal(name) => names.push(name),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => return Err(other(dir, "isn't a plain full path")),
        }
    }
    if !dir.is_absolute() {
        return Err(other(dir, "isn't a full path"));
    }
    let mut folder = open_plain_folder(&root)?;
    visit(&folder, 0)?;
    for (depth, name) in names.into_iter().enumerate() {
        let wide: Vec<u16> = name.encode_wide().collect();
        let next = folder
            .open_child(&wide, true)
            .map_err(|error| refused(dir, &error))?;
        if next.reparse() {
            return Err(Refused::Reparse);
        }
        if !next.folder {
            return Err(other(dir, "has a file on the way"));
        }
        visit(&next, depth + 1)?;
        folder = next;
    }
    Ok(())
}

/// The identity of the regular file `path`, opened as itself; `None` when it isn't there. A
/// reparse point is [`Refused::Reparse`], a folder is refused.
pub(super) fn file_id(path: &Path) -> Result<Option<FileId>, Refused> {
    let handle = match open_itself(path) {
        Ok(handle) => handle,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(other(path, &error.to_string())),
    };
    let fail = |error: io::Error| other(path, &error.to_string());
    let tag: FILE_ATTRIBUTE_TAG_INFO = info(&handle, FileAttributeTagInfo).map_err(fail)?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Refused::Reparse);
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        return Err(other(path, "is a folder, not a file"));
    }
    let id: FILE_ID_INFO = info(&handle, FileIdInfo).map_err(fail)?;
    let basic: FILE_BASIC_INFO = info(&handle, FileBasicInfo).map_err(fail)?;
    let standard: FILE_STANDARD_INFO = info(&handle, FileStandardInfo).map_err(fail)?;
    Ok(Some(FileId {
        volume: id.VolumeSerialNumber,
        id: u128::from_le_bytes(id.FileId.Identifier),
        size: u64::try_from(standard.EndOfFile).unwrap_or(0),
        written: u64::try_from(basic.LastWriteTime).unwrap_or(0),
    }))
}

/// The serial number of the volume holding `path` (`FileIdInfo` of `path` itself).
pub(super) fn volume(path: &Path) -> Result<u64, String> {
    let read = open_itself(path).and_then(|handle| info::<FILE_ID_INFO>(&handle, FileIdInfo));
    read.map(|id| id.VolumeSerialNumber)
        .map_err(|error| format!("reading {}'s volume: {error}", path.display()))
}

/// Every folder and file below `root`, each folder before what it holds, listed through
/// handles each opened relative to its folder; a reparse point anywhere refuses.
pub(super) fn walk(root: &Path) -> Result<Vec<Entry>, Refused> {
    let folder = open_plain_folder(root)?;
    let mut out = Vec::new();
    walk_in(root, &folder, Path::new(""), &mut out)?;
    Ok(out)
}

/// [`walk`] below the open folder `folder`, whose path from the root is `rel`.
fn walk_in(root: &Path, folder: &Object, rel: &Path, out: &mut Vec<Entry>) -> Result<(), Refused> {
    let children = folder.children().map_err(|error| refused(root, &error))?;
    for child in children {
        if child.reparse {
            return Err(Refused::Reparse);
        }
        let path = rel.join(OsString::from_wide(&child.name));
        if !child.folder {
            // Opened as itself for its attributes alone: its 128-bit file ID, the same under every
            // name it has, inside the install or in a tree.
            let file = folder
                .open_child_for(&child.name, 0)
                .map_err(|error| refused(root, &error))?;
            if file.reparse() {
                return Err(Refused::Reparse);
            }
            if file.folder {
                return Err(other(root, "changed while it was listed"));
            }
            let (_volume, id) = file
                .file_id()
                .map_err(|error| other(root, &format!("reading a file's ID: {error}")))?;
            out.push(Entry {
                rel: path,
                folder: false,
                id: u128::from_le_bytes(id),
            });
            continue;
        }
        out.push(Entry {
            rel: path.clone(),
            folder: true,
            id: 0,
        });
        let below = folder
            .open_child(&child.name, true)
            .map_err(|error| refused(root, &error))?;
        if below.reparse() {
            return Err(Refused::Reparse);
        }
        if !below.folder {
            return Err(other(root, "changed while it was listed"));
        }
        walk_in(root, &below, &path, out)?;
    }
    Ok(())
}

/// The names in the folder `dir`, listed through its own handle; none when it is missing. A
/// reparse point there is refused.
pub(super) fn entries(dir: &Path) -> Result<Vec<String>, String> {
    let folder = match Object::open_folder(dir) {
        Ok(folder) => folder,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("opening {}: {error}", dir.display())),
    };
    if folder.reparse() || !folder.folder {
        return Err(format!("{} isn't a plain folder", dir.display()));
    }
    let children = folder
        .children()
        .map_err(|error| format!("listing {}: {error}", dir.display()))?;
    Ok(children
        .iter()
        .map(|child| {
            OsString::from_wide(&child.name)
                .to_string_lossy()
                .into_owned()
        })
        .collect())
}

/// Makes the folder `path` with no descriptor of its own, so it inherits its parent's DACL.
pub(super) fn make_dir(path: &Path) -> Result<(), String> {
    let wide = wide(path)?;
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes.
    unsafe { CreateDirectoryW(PCWSTR(wide.as_ptr()), None) }
        .map_err(|error| format!("making {}: {}", path.display(), os_error(&error)))
}

/// Makes `link` a new name of the file `existing`.
pub(super) fn hard_link(existing: &Path, link: &Path) -> Result<(), String> {
    let (from, to) = (wide(existing)?, wide(link)?);
    // SAFETY: both strings are NUL-terminated and outlive the call; no security attributes
    // (a hard link has its file's).
    unsafe { CreateHardLinkW(PCWSTR(to.as_ptr()), PCWSTR(from.as_ptr()), None) }
        .map_err(|error| format!("linking {}: {}", link.display(), os_error(&error)))
}

/// Copies the file `from` to the new file `to`, refused when `to` is there. `CopyFileExW`
/// copies the data, the streams and the attributes, not the DACL: `to` inherits its folder's.
pub(super) fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    let (source, target) = (wide(from)?, wide(to)?);
    // SAFETY: both strings are NUL-terminated and outlive the call; no progress routine, data
    // or cancel flag.
    unsafe {
        CopyFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            None,
            None,
            None,
            COPY_FILE_FAIL_IF_EXISTS,
        )
    }
    .map_err(|error| format!("copying to {}: {}", to.display(), os_error(&error)))
}

/// Renames the folder `from` to `to`, which must not exist, written through before it returns.
pub(super) fn rename(from: &Path, to: &Path) -> Result<(), String> {
    let (source, target) = (wide(from)?, wide(to)?);
    // SAFETY: both strings are NUL-terminated and outlive the call. Without
    // MOVEFILE_REPLACE_EXISTING an existing `to` fails it.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| format!("renaming {}: {}", from.display(), os_error(&error)))
}

/// Deletes `path` and all below it, a link as itself and never through one, taking back what
/// shuts Administrators out (`admin_copy::take_back`); missing is `Ok`.
pub(super) fn remove_tree(path: &Path) -> Result<(), String> {
    remove_taking_back(&mut NtfsLockDown, path)
        .map_err(|error| format!("deleting {}: {error}", path.display()))
}

/// The whole of the regular file `path`, opened as itself, refused over `cap` bytes.
#[expect(
    clippy::disallowed_methods,
    reason = "reads an exe of the per-machine Git to read its version, never writes it"
)]
pub(super) fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, String> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    let fail = |error: io::Error| format!("reading {}: {error}", path.display());
    let file = File::options()
        .read(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_DELETE).0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(fail)?;
    let meta = file.metadata().map_err(fail)?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || !meta.is_file() {
        return Err(format!("{} isn't a regular file", path.display()));
    }
    let mut image = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut image)
        .map_err(fail)?;
    if u64::try_from(image.len()).unwrap_or(u64::MAX) > cap {
        return Err(format!("{} is larger than its cap", path.display()));
    }
    Ok(image)
}

/// `root` opened as itself to list, refused unless a plain folder.
fn open_plain_folder(root: &Path) -> Result<Object, Refused> {
    let folder = Object::open_folder(root).map_err(|error| refused(root, &error))?;
    if folder.reparse() {
        return Err(Refused::Reparse);
    }
    if !folder.folder {
        return Err(other(root, "isn't a folder"));
    }
    Ok(folder)
}

/// `path` itself (`FILE_FLAG_OPEN_REPARSE_POINT`), a folder too, for its attributes alone.
fn open_itself(path: &Path) -> io::Result<Handle> {
    let wide = verbatim_wide(path)?;
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes or
    // template are passed, and the handle is owned by `Handle` from here.
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map(Handle)
    .map_err(|error| os_error(&error))
}

/// The `class` information of what `handle` is open on; `T` is that class's struct.
pub(super) fn info<T: Default>(handle: &Handle, class: FILE_INFO_BY_HANDLE_CLASS) -> io::Result<T> {
    let mut value = T::default();
    let size = u32::try_from(size_of::<T>()).map_err(io::Error::other)?;
    // SAFETY: the handle is open with FILE_READ_ATTRIBUTES; `value` is a live, writable `T`
    // of `size` bytes, the struct `class` fills.
    unsafe { GetFileInformationByHandleEx(handle.0, class, (&raw mut value).cast(), size) }
        .map_err(|error| os_error(&error))?;
    Ok(value)
}

/// `path` in the `\\?\` form, NUL-terminated, or its words.
fn wide(path: &Path) -> Result<Vec<u16>, String> {
    verbatim_wide(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// A failed open on the way to or below `path`: a reparse point met is [`Refused::Reparse`].
fn refused(path: &Path, error: &io::Error) -> Refused {
    if reparse_refused(error) {
        Refused::Reparse
    } else {
        other(path, &error.to_string())
    }
}

/// [`Refused::Other`], naming `path`.
fn other(path: &Path, why: &str) -> Refused {
    Refused::Other(format!("{}: {why}", path.display()))
}
