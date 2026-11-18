//! macOS: a file's ACL, through the `acl(3)` calls of libSystem, for [`super::keep`] and the
//! temp file's removal, always through a descriptor; and its birth time, through
//! `fsetattrlist`. macOS ACLs are not extended attributes `listxattr` shows. Not through
//! `fcopyfile` with `COPYFILE_ACL`: that also hands the source's quarantine to libquarantine,
//! which writes a fresh stamp (the time of the call, no agent) over the copied one.
#![expect(unsafe_code, reason = "FFI to the acl(3) calls and fsetattrlist")]

#[cfg(test)]
use std::ffi::{CString, c_char};
use std::ffi::{c_int, c_void};
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd as _;
#[cfg(test)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(test)]
use std::path::Path;

/// `acl_t`, `acl_entry_t` and `acl_permset_t` (`<sys/acl.h>`): opaque pointers.
type AclT = *mut c_void;
type EntryT = *mut c_void;
type PermsetT = *mut c_void;

/// `ACL_TYPE_EXTENDED`: macOS's one ACL type.
const ACL_TYPE_EXTENDED: c_int = 0x0000_0100;
/// `ACL_EXTENDED_ALLOW`, `ACL_EXTENDED_DENY` (`acl_tag_t`).
const ACL_EXTENDED_ALLOW: c_int = 1;
#[cfg(test)]
const ACL_EXTENDED_DENY: c_int = 2;
/// `ACL_DELETE`, `ACL_DELETE_CHILD` (`acl_perm_t`).
pub(super) const ACL_DELETE: c_int = 1 << 4;
#[cfg(test)]
pub(super) const ACL_DELETE_CHILD: c_int = 1 << 6;
/// `ACL_READ_DATA` (`acl_perm_t`).
#[cfg(test)]
const ACL_READ_DATA: c_int = 1 << 1;

unsafe extern "C" {
    fn acl_init(count: c_int) -> AclT;
    fn acl_free(obj: *mut c_void) -> c_int;
    fn acl_get_fd_np(fd: c_int, kind: c_int) -> AclT;
    fn acl_set_fd_np(fd: c_int, acl: AclT, kind: c_int) -> c_int;
    #[cfg(test)]
    fn acl_set_link_np(path: *const c_char, kind: c_int, acl: AclT) -> c_int;
    fn acl_create_entry(acl: *mut AclT, entry: *mut EntryT) -> c_int;
    fn acl_set_tag_type(entry: EntryT, tag: c_int) -> c_int;
    fn acl_set_qualifier(entry: EntryT, qualifier: *const c_void) -> c_int;
    fn acl_get_permset(entry: EntryT, permset: *mut PermsetT) -> c_int;
    fn acl_add_perm(permset: PermsetT, perm: c_int) -> c_int;
    fn acl_set_permset(entry: EntryT, permset: PermsetT) -> c_int;
    fn mbr_uid_to_uuid(uid: libc::uid_t, uuid: *mut u8) -> c_int;
    #[cfg(test)]
    fn mbr_gid_to_uuid(gid: libc::gid_t, uuid: *mut u8) -> c_int;
    #[cfg(test)]
    fn acl_to_text(acl: AclT, len: *mut isize) -> *mut c_char;
}

/// An `acl_t` this module owns, freed when dropped: a file's ACL held in memory.
pub(super) struct Acl(AclT);

impl std::fmt::Debug for Acl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Acl")
    }
}

impl Drop for Acl {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `acl_init` or `acl_get_fd_np`, and is freed once.
        unsafe { acl_free(self.0) };
    }
}

/// `Err` of errno when `done` is not 0.
fn check(done: c_int) -> io::Result<()> {
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The ACL of the open file `file`, read into memory; `None` when it has none.
pub(super) fn read_acl(file: &File) -> io::Result<Option<Acl>> {
    // SAFETY: the descriptor is open for the whole call.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        let err = io::Error::last_os_error();
        return if err.raw_os_error() == Some(libc::ENOENT) {
            Ok(None)
        } else {
            Err(err)
        };
    }
    Ok(Some(Acl(acl)))
}

/// Sets the ACL of the open file `file` to `acl`, through its descriptor.
pub(super) fn set_acl(file: &File, acl: &Acl) -> io::Result<()> {
    // SAFETY: the descriptor is open and `acl` is a valid ACL for the whole call, which keeps
    // no pointer to it.
    check(unsafe { acl_set_fd_np(file.as_raw_fd(), acl.0, ACL_TYPE_EXTENDED) })
}

/// Sets the birth time ("Created" in Finder) of the open file `file` to `secs` and `nanos`
/// since the epoch.
pub(super) fn set_birth_time(file: &File, secs: i64, nanos: i64) -> io::Result<()> {
    let mut list = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_CRTIME,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    let mut time = libc::timespec {
        tv_sec: secs,
        tv_nsec: nanos,
    };
    // SAFETY: the descriptor is open, and `list` and `time` (the one attribute it names, of
    // the size passed) outlive the call, which keeps no pointer to them.
    check(unsafe {
        libc::fsetattrlist(
            file.as_raw_fd(),
            (&raw mut list).cast(),
            (&raw mut time).cast(),
            size_of::<libc::timespec>(),
            0,
        )
    })
}

/// Gives the open file `file` an ACL of one entry, the current user allowed to delete it, in
/// place of the one it had, through its descriptor: a temp file a deny-delete entry (inherited,
/// or the folder's `delete_child`) keeps from its removal can then go. The caller has checked
/// the descriptor is that temp file, with one link ([`super::temps`]).
pub(super) fn allow_delete(file: &File) -> io::Result<()> {
    let acl = entries(&[(ACL_EXTENDED_ALLOW, me()?, ACL_DELETE)])?;
    set_acl(file, &acl)
}

/// Tests: the current user denied `perm` on `path`, as its whole ACL.
#[cfg(test)]
pub(super) fn deny(path: &Path, perm: c_int) -> io::Result<()> {
    set_entries(path, &[(ACL_EXTENDED_DENY, me()?, perm)])
}

/// Tests: the ACL of `path` set to `everyone` (gid 12) denied `delete`, then the group
/// `daemon` (gid 1) denied reading: a lawyer's file an agent should not reach through a link.
#[cfg(test)]
pub(super) fn deny_delete_and_read(path: &Path) -> io::Result<()> {
    set_entries(
        path,
        &[
            (ACL_EXTENDED_DENY, group(12)?, ACL_DELETE),
            (ACL_EXTENDED_DENY, group(1)?, ACL_READ_DATA),
        ],
    )
}

/// Tests: the ACL of `path` set to `everyone` (gid 12) denied `delete`, what
/// `chmod +a "everyone deny delete"` makes of a file with none: a rename over it is refused.
#[cfg(test)]
pub(crate) fn deny_everyone_delete(path: &Path) -> io::Result<()> {
    set_entries(path, &[(ACL_EXTENDED_DENY, group(12)?, ACL_DELETE)])
}

/// Tests: the UUID of the group `gid`.
#[cfg(test)]
fn group(gid: libc::gid_t) -> io::Result<[u8; 16]> {
    let mut uuid = [0_u8; 16];
    // SAFETY: `uuid` is the 16 bytes the call writes. It answers an errno, not -1.
    let found = unsafe { mbr_gid_to_uuid(gid, uuid.as_mut_ptr()) };
    if found == 0 {
        Ok(uuid)
    } else {
        Err(io::Error::from_raw_os_error(found))
    }
}

/// Tests: the ACL of `path` set to the current user allowed `delete`, then the group
/// `everyone` (gid 12) denied it.
#[cfg(test)]
pub(super) fn allow_me_deny_everyone(path: &Path) -> io::Result<()> {
    set_entries(
        path,
        &[
            (ACL_EXTENDED_ALLOW, me()?, ACL_DELETE),
            (ACL_EXTENDED_DENY, group(12)?, ACL_DELETE),
        ],
    )
}

/// Tests: the ACL of the file `file` as `acl_to_text` writes it; empty for none.
#[cfg(test)]
pub(crate) fn acl_text(file: &File) -> String {
    // SAFETY: the descriptor is open for the whole call.
    let acl = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if acl.is_null() {
        return String::new();
    }
    let acl = Acl(acl);
    let mut len = 0_isize;
    // SAFETY: `acl` is valid; the answer is a NUL-terminated string the caller frees.
    let text = unsafe { acl_to_text(acl.0, &raw mut len) };
    assert!(!text.is_null(), "{}", io::Error::last_os_error());
    // SAFETY: `text` is NUL-terminated, read before it is freed once.
    let owned = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: `text` came from `acl_to_text`.
    unsafe { acl_free(text.cast()) };
    owned
}

/// The current user's UUID, the qualifier of an ACL entry.
fn me() -> io::Result<[u8; 16]> {
    let mut uuid = [0_u8; 16];
    // SAFETY: `uuid` is the 16 bytes the call writes. It answers an errno, not -1.
    let found = unsafe { mbr_uid_to_uuid(libc::geteuid(), uuid.as_mut_ptr()) };
    if found == 0 {
        Ok(uuid)
    } else {
        Err(io::Error::from_raw_os_error(found))
    }
}

/// Tests: no ACL entries on `path`.
#[cfg(test)]
pub(crate) fn clear_acl(path: &Path) -> io::Result<()> {
    // SAFETY: no pointers in; the result is owned by `Acl`.
    let acl = Acl(unsafe { acl_init(0) });
    set_link(path, &acl)
}

/// Tests: sets the ACL of `path` (not through a symlink) to [`entries`].
#[cfg(test)]
fn set_entries(path: &Path, list: &[(c_int, [u8; 16], c_int)]) -> io::Result<()> {
    set_link(path, &entries(list)?)
}

/// An ACL of the entries `list`, in order: each a tag, a user or group UUID and a permission.
fn entries(list: &[(c_int, [u8; 16], c_int)]) -> io::Result<Acl> {
    // SAFETY: no pointers in; a null answer is checked below.
    let raw = unsafe { acl_init(c_int::try_from(list.len()).unwrap_or(c_int::MAX)) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut acl = Acl(raw);
    for (tag, uuid, perm) in list {
        let mut entry: EntryT = std::ptr::null_mut();
        let mut permset: PermsetT = std::ptr::null_mut();
        // SAFETY: `acl.0` is a valid ACL the call may move (it writes the new pointer back),
        // `entry` and `permset` are written by the calls before use, and `uuid` outlives them.
        unsafe {
            check(acl_create_entry(&raw mut acl.0, &raw mut entry))?;
            check(acl_set_tag_type(entry, *tag))?;
            check(acl_set_qualifier(entry, uuid.as_ptr().cast()))?;
            check(acl_get_permset(entry, &raw mut permset))?;
            check(acl_add_perm(permset, *perm))?;
            check(acl_set_permset(entry, permset))?;
        }
    }
    Ok(acl)
}

/// Tests: sets the ACL of `path` (not through a symlink) to `acl`.
#[cfg(test)]
fn set_link(path: &Path, acl: &Acl) -> io::Result<()> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?;
    // SAFETY: `path` is NUL-terminated and `acl` valid for the whole call, which keeps no
    // pointer to either.
    check(unsafe { acl_set_link_np(path.as_ptr(), ACL_TYPE_EXTENDED, acl.0) })
}
