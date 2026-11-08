//! What the Git trees make below `git\`: a source copy's or a tree's folders and files
//! ([`copy_tree`], [`link_tree`]) and a completion marker ([`write_marker`]). Every folder on
//! either side is opened, or made, relative to its parent's handle and kept open while what it
//! holds is done (`nt::Open`, `OBJ_DONT_REPARSE`, `FILE_OPEN_REPARSE_POINT`); every source file
//! is opened relative to its walked folder's handle as itself and checked to be the file the
//! walk saw. What is made gets its owner (and a marker its DACL) in the descriptor it is made
//! with; nothing's security is set afterwards, and a linked file keeps the one it shares.
#![expect(
    unsafe_code,
    reason = "FFI to ReadFile, WriteFile, FlushFileBuffers and SetFileInformationByHandle"
)]

use std::io;
use std::mem::offset_of;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

use windows::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_LINK_INFORMATION, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
    FILE_SEQUENTIAL_ONLY, FileLinkInformation, NTCREATEFILE_CREATE_OPTIONS,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_BASIC_INFO, FILE_ID_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
    FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO,
    FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FileAttributeTagInfo, FileBasicInfo,
    FileIdInfo, FileStandardInfo, FlushFileBuffers, ReadFile, SetFileInformationByHandle,
    WriteFile,
};

use super::super::{Entry, MARKER, Refused};
use super::files::{info, open_plain_folder, other};
use super::nt::{Descriptor, OWNER_SYSTEM, Open, set_information};
use crate::windows::acl::object::{Handle, Object, os_error, reparse_refused, words};
use crate::windows::acl::privilege::{BACKUP_RESTORE, Privileges};

/// Every share mode.
const SHARE_ALL: FILE_SHARE_MODE =
    FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);
/// A folder walked or filled: listed, and the root of what is opened in it.
const FOLDER_ACCESS: u32 = FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0;
/// How much of a file each read copies.
const CHUNK: usize = 1 << 20;

/// One folder of the walk open on both sides: `rel` from the roots.
struct Level {
    rel: PathBuf,
    from: Handle,
    to: Handle,
}

/// Into the empty folder `to`, `entries` of the folder `from`, each file's data copied into a
/// new file owner SYSTEM, its DACL inherited (`TreeFs::copy_tree`).
pub(super) fn copy_tree(from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused> {
    put_tree(from, entries, to, |level, name, entry, owner| {
        let source = open_source(level, name, entry, FILE_READ_DATA.0, read_share())?;
        let fail = |why: &str| other(&from.join(&entry.rel), why);
        let target = Open {
            access: FILE_WRITE_DATA.0 | FILE_WRITE_ATTRIBUTES.0 | FILE_READ_ATTRIBUTES.0,
            share: FILE_SHARE_MODE(0),
            disposition: FILE_CREATE,
            options: FILE_NON_DIRECTORY_FILE | FILE_SEQUENTIAL_ONLY,
        }
        .at(&level.to, name, Some(owner))
        .map_err(|error| fail(&format!("making its copy: {error}")))?;
        let copied = copy_data(&source, &target).map_err(|error| fail(&error.to_string()))?;
        if copied != entry.size {
            return Err(Refused::Changed);
        }
        let times = FILE_BASIC_INFO {
            LastWriteTime: i64::try_from(entry.written).unwrap_or(0),
            ..FILE_BASIC_INFO::default()
        };
        let size = u32::try_from(size_of::<FILE_BASIC_INFO>()).unwrap_or(0);
        // SAFETY: the handle is open with FILE_WRITE_ATTRIBUTES; `times` is a live
        // FILE_BASIC_INFO of `size` bytes, whose zero times are left as they are.
        unsafe {
            SetFileInformationByHandle(target.0, FileBasicInfo, (&raw const times).cast(), size)
        }
        .map_err(|error| fail(&format!("setting its write time: {}", os_error(&error))))?;
        // SAFETY: the handle is open with FILE_WRITE_DATA.
        unsafe { FlushFileBuffers(target.0) }
            .map_err(|error| fail(&format!("flushing its copy: {}", os_error(&error))))
    })
}

/// [`copy_tree`], but each file is given a new name in its destination folder
/// (`FileLinkInformation`, relative to that folder's handle), sharing its security.
pub(super) fn link_tree(from: &Path, entries: &[Entry], to: &Path) -> Result<(), Refused> {
    put_tree(from, entries, to, |level, name, entry, _owner| {
        // `CreateHardLinkW` opens its source for FILE_WRITE_ATTRIBUTES; so does this.
        let source = open_source(level, name, entry, FILE_WRITE_ATTRIBUTES.0, SHARE_ALL)?;
        link(&source, &level.to, name)
            .map_err(|error| other(&from.join(&entry.rel), &format!("linking it: {error}")))
    })
}

/// Walks `entries` (a walk's, parents first) with each folder open on both sides: a folder is
/// opened in its source folder and made in its destination folder, owner SYSTEM; a file is
/// handed to `put` with its folders' [`Level`], its name and the owner's descriptor. Enables
/// `SeBackupPrivilege` and `SeRestorePrivilege` (SYSTEM as another's owner) for the call.
fn put_tree(
    from: &Path,
    entries: &[Entry],
    to: &Path,
    mut put: impl FnMut(&Level, &[u16], &Entry, &Descriptor) -> Result<(), Refused>,
) -> Result<(), Refused> {
    let _held = Privileges::enable(&BACKUP_RESTORE)
        .map_err(|error| other(to, &format!("enabling backup and restore: {error}")))?;
    let owner = Descriptor::from_sddl(OWNER_SYSTEM).map_err(|why| other(to, &why))?;
    let mut levels = vec![Level {
        rel: PathBuf::new(),
        from: open_root(from)?,
        to: open_root(to)?,
    }];
    for entry in entries {
        let (Some(parent), Some(name)) = (entry.rel.parent(), entry.rel.file_name()) else {
            return Err(other(from, "a walk's entry has no name"));
        };
        while levels
            .last()
            .is_some_and(|level| level.rel.as_path() != parent)
        {
            levels.pop();
        }
        let Some(level) = levels.last() else {
            return Err(other(from, "a walk's entry comes before its folder"));
        };
        if entry.reparse {
            return Err(Refused::Reparse);
        }
        let name: Vec<u16> = name.encode_wide().collect();
        if !entry.folder {
            put(level, &name, entry, &owner)?;
            continue;
        }
        let fail = |why: String| other(&to.join(&entry.rel), &why);
        let source = open_folder_in(&level.from, &name, &from.join(&entry.rel))?;
        let made = Open {
            access: FOLDER_ACCESS,
            share: SHARE_ALL,
            disposition: FILE_CREATE,
            options: FILE_DIRECTORY_FILE,
        }
        .at(&level.to, &name, Some(&owner))
        .map_err(|error| fail(format!("making it: {error}")))?;
        levels.push(Level {
            rel: entry.rel.clone(),
            from: source,
            to: made,
        });
    }
    Ok(())
}

/// The plain folder `path`, opened relative to its parent's handle.
fn open_root(path: &Path) -> Result<Handle, Refused> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(other(path, "has no parent"));
    };
    let above = open_plain_folder(parent)?;
    let name: Vec<u16> = name.encode_wide().collect();
    open_folder_in(above.handle(), &name, path)
}

/// The folder `name` in the folder `dir`, opened as itself relative to its handle to list and
/// to open in: a reparse point is [`Refused::Reparse`], a file is refused. `path` names it.
fn open_folder_in(dir: &Handle, name: &[u16], path: &Path) -> Result<Handle, Refused> {
    let opened = Open {
        access: FOLDER_ACCESS,
        share: SHARE_ALL,
        disposition: FILE_OPEN,
        options: NTCREATEFILE_CREATE_OPTIONS(0),
    }
    .at(dir, name, None)
    .map_err(|error| opened_error(path, &error))?;
    let tag: FILE_ATTRIBUTE_TAG_INFO =
        info(&opened, FileAttributeTagInfo).map_err(|error| other(path, &error.to_string()))?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Refused::Reparse);
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(other(path, "isn't a folder"));
    }
    Ok(opened)
}

/// The file `name` of `level`'s source folder, opened as itself relative to its handle for
/// `rights`, shared as `share`; refused unless a plain file whose 128-bit ID, size and write
/// time are `entry`'s ([`Refused::Changed`]). Its words name it from the walk's root.
fn open_source(
    level: &Level,
    name: &[u16],
    entry: &Entry,
    rights: u32,
    share: FILE_SHARE_MODE,
) -> Result<Handle, Refused> {
    let path = entry.rel.as_path();
    let source = Open {
        access: rights | FILE_READ_ATTRIBUTES.0,
        share,
        disposition: FILE_OPEN,
        options: NTCREATEFILE_CREATE_OPTIONS(0),
    }
    .at(&level.from, name, None)
    .map_err(|error| opened_error(path, &error))?;
    let fail = |error: io::Error| other(path, &error.to_string());
    let tag: FILE_ATTRIBUTE_TAG_INFO = info(&source, FileAttributeTagInfo).map_err(fail)?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Refused::Reparse);
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        return Err(other(path, "is a folder, not a file"));
    }
    let id: FILE_ID_INFO = info(&source, FileIdInfo).map_err(fail)?;
    let basic: FILE_BASIC_INFO = info(&source, FileBasicInfo).map_err(fail)?;
    let standard: FILE_STANDARD_INFO = info(&source, FileStandardInfo).map_err(fail)?;
    let same = u128::from_le_bytes(id.FileId.Identifier) == entry.id
        && u64::try_from(standard.EndOfFile).ok() == Some(entry.size)
        && u64::try_from(basic.LastWriteTime).ok() == Some(entry.written);
    if !same {
        return Err(Refused::Changed);
    }
    Ok(source)
}

/// What a copied file's source shares: readers and a rename or delete, never a writer.
fn read_share() -> FILE_SHARE_MODE {
    FILE_SHARE_READ | FILE_SHARE_DELETE
}

/// A failed open of `path`: a reparse point met on the way is [`Refused::Reparse`].
fn opened_error(path: &Path, error: &io::Error) -> Refused {
    if reparse_refused(error) {
        Refused::Reparse
    } else {
        other(path, &error.to_string())
    }
}

/// Copies everything `source` holds into `target` from where each is; how many bytes.
pub(super) fn copy_data(source: &Handle, target: &Handle) -> io::Result<u64> {
    let mut buffer = vec![0_u8; CHUNK];
    let mut total = 0_u64;
    loop {
        let mut read = 0_u32;
        // SAFETY: the handle is open for synchronous reads with FILE_READ_DATA; `buffer` is a
        // live, writable slice and `read` a live out-pointer.
        unsafe { ReadFile(source.0, Some(&mut buffer), Some(&raw mut read), None) }
            .map_err(|error| os_error(&error))?;
        if read == 0 {
            return Ok(total);
        }
        let read = usize::try_from(read).map_err(io::Error::other)?;
        write_all(target, buffer.get(..read).unwrap_or_default())?;
        total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
}

/// What `source` holds from where it is, refused past `cap` bytes.
pub(super) fn read_all(source: &Handle, cap: u64) -> io::Result<Vec<u8>> {
    let mut buffer = vec![0_u8; CHUNK];
    let mut all = Vec::new();
    loop {
        let mut read = 0_u32;
        // SAFETY: the handle is open for synchronous reads with FILE_READ_DATA; `buffer` is a
        // live, writable slice and `read` a live out-pointer.
        unsafe { ReadFile(source.0, Some(&mut buffer), Some(&raw mut read), None) }
            .map_err(|error| os_error(&error))?;
        if read == 0 {
            return Ok(all);
        }
        let read = usize::try_from(read).map_err(io::Error::other)?;
        all.extend_from_slice(buffer.get(..read).unwrap_or_default());
        if u64::try_from(all.len()).unwrap_or(u64::MAX) > cap {
            return Err(io::Error::other("larger than its cap"));
        }
    }
}

/// Writes all of `bytes` to `target` from where it is.
pub(super) fn write_all(target: &Handle, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let mut written = 0_u32;
        // SAFETY: the handle is open for synchronous writes with FILE_WRITE_DATA; `bytes` is a
        // live slice and `written` a live out-pointer.
        unsafe { WriteFile(target.0, Some(bytes), Some(&raw mut written), None) }
            .map_err(|error| os_error(&error))?;
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "a write wrote nothing",
            ));
        }
        let written = usize::try_from(written).map_err(io::Error::other)?;
        bytes = bytes.get(written..).unwrap_or_default();
    }
    Ok(())
}

/// Gives the file `source` is open on the new name `name` in the folder `folder`, relative to
/// its handle (`FileLinkInformation`, never replacing what is there). The variable-length
/// `FILE_LINK_INFORMATION` is laid out by its fields' offsets in an 8-byte-aligned buffer.
fn link(source: &Handle, folder: &Handle, name: &[u16]) -> io::Result<()> {
    let at = offset_of!(FILE_LINK_INFORMATION, FileName);
    let length = at + name.len() * 2;
    let mut bytes = vec![0_u8; length.max(size_of::<FILE_LINK_INFORMATION>())];
    let mut put = |offset: usize, data: &[u8]| {
        if let Some(slot) = bytes.get_mut(offset..offset + data.len()) {
            slot.copy_from_slice(data);
        }
    };
    // `ReplaceIfExists` (the union's first byte) stays 0: an existing name fails the link.
    put(
        offset_of!(FILE_LINK_INFORMATION, RootDirectory),
        &folder.0.0.addr().to_ne_bytes(),
    );
    let name_bytes = u32::try_from(name.len() * 2).map_err(io::Error::other)?;
    put(
        offset_of!(FILE_LINK_INFORMATION, FileNameLength),
        &name_bytes.to_ne_bytes(),
    );
    let wide: Vec<u8> = name.iter().flat_map(|unit| unit.to_ne_bytes()).collect();
    put(at, &wide);
    set_information(source, FileLinkInformation, &words(&bytes), length)
}

/// Makes [`MARKER`] in the folder `dir`, relative to its handle, with the descriptor `sddl`,
/// holding `text`, flushed before it returns. `SeRestorePrivilege` is enabled to give it
/// SYSTEM as its owner.
pub(super) fn write_marker(dir: &Path, text: &str, sddl: &str) -> Result<(), String> {
    let fail = |why: &dyn std::fmt::Display| format!("writing {}'s marker: {why}", dir.display());
    let _restore = Privileges::enable(&BACKUP_RESTORE).map_err(|error| fail(&error))?;
    let descriptor = Descriptor::from_sddl(sddl).map_err(|why| fail(&why))?;
    let folder = Object::open_folder(dir).map_err(|error| fail(&error))?;
    if folder.reparse() || !folder.folder {
        return Err(fail(&"it isn't a plain folder"));
    }
    let name: Vec<u16> = MARKER.encode_utf16().collect();
    let file = Open {
        access: FILE_WRITE_DATA.0 | FILE_READ_ATTRIBUTES.0,
        share: FILE_SHARE_MODE(0),
        disposition: FILE_CREATE,
        options: FILE_NON_DIRECTORY_FILE,
    }
    .at(folder.handle(), &name, Some(&descriptor))
    .map_err(|error| fail(&error))?;
    write_all(&file, text.as_bytes()).map_err(|error| fail(&error))?;
    // SAFETY: the handle is open with FILE_WRITE_DATA.
    unsafe { FlushFileBuffers(file.0) }.map_err(|error| fail(&os_error(&error)))
}
