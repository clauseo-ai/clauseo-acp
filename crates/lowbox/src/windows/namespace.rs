//! The package's object namespace: built by the owner, handed to the guard, checked there.
//!
//! The owner (the lawyer's outer process, in the desktop session) makes six objects per launch,
//! as the W2 (j) recipe did (`/tmp/stagec/w2-j-recipe.md`, step 1); see `crate::layout` for their
//! names and descriptors. They live while a handle or a `LowBox` token holds them, so the owner
//! keeps its [`NamespaceHandles`] until the browser has exited.
#![expect(
    unsafe_code,
    reason = "FFI to the Nt object calls, the token and handle calls"
)]

use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle};

use ::windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF, NTCREATEFILE_CREATE_DISPOSITION, NtCreateFile,
    NtOpenDirectoryObject, NtOpenSymbolicLinkObject,
};
use ::windows::Win32::Foundation::{
    DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS,
    SetHandleInformation,
};
use ::windows::Win32::Storage::FileSystem::{
    FILE_ACCESS_RIGHTS, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use ::windows::Win32::System::IO::IO_STATUS_BLOCK;
use ::windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use ::windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};
use clauseo_process::SpawnLock;

use super::nt::{Descriptor, NtCreateDirectoryObjectEx, NtCreateSymbolicLinkObject, ObjectName};
use super::probe::{ThisProcess, handle};
use super::{nt_ok, token, win32};
use crate::error::Error;
use crate::handles::{
    self, Expected, HANDED_COUNT, HANDLE_COUNT, ObjectKind, Slot, format_handle_arg,
};
use crate::layout;
use crate::package::{PackageName, Sid, package_sid};

/// `DIRECTORY_ALL_ACCESS`: what the owner holds its directories with.
const DIRECTORY_ALL_ACCESS: u32 = 0x000F_000F;
/// `DIRECTORY_QUERY | DIRECTORY_TRAVERSE`: enough to shadow a directory.
const DIRECTORY_SHADOW: u32 = 0x0000_0003;
/// `READ_CONTROL | DIRECTORY_QUERY | DIRECTORY_TRAVERSE`: the guard's copy of a directory, which
/// it only checks (type, name, owner) and hands to the token call; no create, delete or DACL
/// rights. Narrower than the recipe's (all access): the proof row checks the token call takes it.
const DIRECTORY_GUARD: u32 = 0x0002_0003;
/// `SYMBOLIC_LINK_ALL_ACCESS`: what the owner holds its links with.
const LINK_ALL_ACCESS: u32 = 0x000F_0001;
/// `READ_CONTROL | SYMBOLIC_LINK_QUERY`: what the guard opens a link with.
const LINK_OPEN: u32 = 0x0002_0001;
/// `FILE_GENERIC_READ` with `FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY`: the owner's pipe folder.
const PIPES_OWNER: u32 = 0x0012_008F;
/// `FILE_GENERIC_READ`: the guard's pipe folder.
const PIPES_OPEN: u32 = 0x0012_0089;
/// `FILE_CREATED`: `IO_STATUS_BLOCK.Information` when `FILE_OPEN_IF` made the file.
const FILE_CREATED: usize = 2;

/// The owner's side: builds a package's namespace.
#[derive(Debug, Clone, Copy)]
pub struct Namespace;

/// The six handles of a package's namespace, in [`Slot::ALL`]'s order, and the package SID.
#[derive(Debug)]
pub struct NamespaceHandles {
    /// The package SID. Never logged (its `Debug` is hidden).
    package: Sid,
    /// The handles, one per slot.
    handles: [OwnedHandle; HANDLE_COUNT],
}

/// The namespace's handed-over handles (every slot in [`Slot::HANDED`]) made inheritable for
/// one launch; dropped, they are not any more.
/// Holds a borrow of the spawn lock, so it cannot outlive it.
#[derive(Debug)]
#[must_use = "the handles stop being inheritable when this is dropped"]
pub struct Inheritable<'a> {
    /// The handles.
    handles: &'a NamespaceHandles,
    /// How many of them, from the first, are on.
    on: usize,
    /// The spawn lock the caller holds.
    _lock: &'a SpawnLock,
}

impl Drop for Inheritable<'_> {
    fn drop(&mut self) {
        for h in self.handles.handles().into_iter().take(self.on) {
            // A failure leaves the flag on, which only a start without a handle list would
            // notice; nothing to undo here. `clauseo_process::spawn_lock()` is taken by
            // `spawn_in_tree` and lowbox's own starts. `mcp/src/browser_guard/job/spawn.rs`
            // must take the same lock around its inheritable handles when mcp adopts lowbox (a
            // follow-up). Process starts by third-party code inside the host are a residual.
            let _left_on = set_flag(h, false);
        }
    }
}

impl Namespace {
    /// Makes the namespace of `package` in session `session` for the browser account
    /// `browser_user`: the package directory shadowing the session's `BaseNamedObjects`, its
    /// `RPC Control`, `Global`, `Local` and `Session`, and the package's pipe folder, each with
    /// the recipe's descriptor naming SYSTEM, this process's user (the owner), the browser
    /// account and the package, and this process's user as the owner (`O:`), which the guard's
    /// [`NamespaceHandles::from_inherited`] checks.
    ///
    /// Every object must be new: the directories and links fail with
    /// `STATUS_OBJECT_NAME_COLLISION` (`0xC0000035`) while an earlier namespace of the package is
    /// alive, and a pipe folder that was already there (anyone's, with any descriptor) fails with
    /// [`Error::Namespace`], as does one whose owner is not this process's user. `session` must
    /// be this process's own ([`Namespace::current_session`]).
    pub fn build(
        package: &PackageName,
        session: u32,
        browser_user: &Sid,
    ) -> Result<NamespaceHandles, Error> {
        if session != Namespace::current_session()? {
            return Err(Error::Namespace {
                reason: "the session is not this process's own",
            });
        }
        let sid = package_sid(package);
        let owner = token::current_user()?;
        let objects = Descriptor::from_sddl(&layout::directory_sddl(&owner, browser_user, &sid))?;
        let pipes = Descriptor::from_sddl(&layout::pipe_folder_sddl(&owner, browser_user, &sid))?;
        let target = ObjectName::new(&layout::directory(session, &sid))?;
        let mut made = Vec::with_capacity(HANDLE_COUNT);
        for slot in Slot::ALL {
            let name = ObjectName::new(&layout::object_name(slot, session, &sid))?;
            let handle = match (slot.kind(), layout::shadow(slot, session)) {
                (ObjectKind::Directory, Some(shadow)) => {
                    let shadow = open_directory(&ObjectName::new(&shadow)?, DIRECTORY_SHADOW)?;
                    create_directory(&name, &objects, &shadow)?
                }
                (ObjectKind::SymbolicLink, _) => create_link(&name, &objects, &target)?,
                _ => new_pipe_folder(&name, &pipes, &owner)?,
            };
            made.push(handle);
        }
        NamespaceHandles::new(sid, made)
    }

    /// This process's session, for [`Namespace::build`].
    pub fn current_session() -> Result<u32, Error> {
        let mut session = 0_u32;
        // SAFETY: both calls only read this process's id and write the session into `session`.
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session) }
            .map_err(|e| win32("ProcessIdToSessionId", &e))?;
        Ok(session)
    }
}

impl NamespaceHandles {
    /// The handles from `made`, which holds one per slot.
    pub(super) fn new(package: Sid, made: Vec<OwnedHandle>) -> Result<NamespaceHandles, Error> {
        let handles = <[OwnedHandle; HANDLE_COUNT]>::try_from(made).map_err(|_wrong_count| {
            Error::HandleList {
                reason: "not exactly six values",
            }
        })?;
        Ok(NamespaceHandles { package, handles })
    }

    /// Opens the namespace of `package` in session `session` by name, as the recipe's
    /// browser-account side did (`w2inner`), with the access it used.
    pub fn open(package: &PackageName, session: u32) -> Result<NamespaceHandles, Error> {
        let sid = package_sid(package);
        let mut opened = Vec::with_capacity(HANDLE_COUNT);
        for slot in Slot::ALL {
            let name = ObjectName::new(&layout::object_name(slot, session, &sid))?;
            opened.push(match slot.kind() {
                ObjectKind::Directory => open_directory(&name, DIRECTORY_ALL_ACCESS)?,
                ObjectKind::SymbolicLink => open_link(&name)?,
                ObjectKind::File => pipe_folder(&name, None, PIPES_OPEN, FILE_OPEN)?.0,
            });
        }
        NamespaceHandles::new(sid, opened)
    }

    /// The guard's side: takes over the handle `values` (from `--lowbox-handles`, see
    /// [`crate::parse_handle_arg`]) that the owner gave this process for `package`, one per
    /// slot in [`Slot::HANDED`], and opens the package's pipe folder itself.
    ///
    /// Each value must be an open handle of its slot's type, name its slot's object in
    /// `package`'s namespace in this process's own session, and be owned by `owner`, the user
    /// who built it (told apart from the values, like the package name); none may be this
    /// process's standard input, output or error.
    ///
    /// The pipe folder is not handed over, because a handle to it cannot be checked:
    /// `NtQueryObject` names any NPFS prefix handle just `\Device\NamedPipe`. This process opens
    /// `\Device\NamedPipe\Sessions\<n>\AppContainerNamedObjects\<package SID>` by that name
    /// (`FILE_OPEN` with `FILE_DIRECTORY_FILE`, which NPFS answers from its prefix of exactly
    /// that name and never creates), as the W2 (j) recipe's browser-account side did, and checks
    /// through the handle it got that the folder is owned by `owner` and has exactly the ACEs
    /// the owner sets (`crate::layout`'s F1 for `owner`, this process's user as the browser
    /// account, and the package). The owner made the folder new ([`Namespace::build`] refuses
    /// one that was already there) and holds it open until the browser has exited, so the name
    /// cannot meanwhile pass to a folder of someone else's.
    ///
    /// The first check that fails fails the whole call, and no value is taken. The handles taken
    /// are made non-inheritable, so only a launch that lists them passes them on.
    pub fn from_inherited(
        package: &PackageName,
        owner: &Sid,
        values: &[usize],
    ) -> Result<NamespaceHandles, Error> {
        let sid = package_sid(package);
        let session = Namespace::current_session()?;
        let expected = Expected {
            package: &sid,
            session,
            owner,
        };
        handles::check(&ThisProcess, values, &expected)?;
        let pipes = open_own_pipe_folder(&expected)?;
        let mut taken: Vec<OwnedHandle> = values
            .iter()
            // SAFETY: each value was just checked to be an open handle of this process, of the
            // slot's type and name; the owner gave it to this process to keep, so it is owned
            // once, here.
            .map(|v| unsafe { OwnedHandle::from_raw_handle(handle(*v).0) })
            .collect();
        taken.push(pipes);
        let handles = NamespaceHandles::new(sid, taken)?;
        handles.set_inherit(false)?;
        Ok(handles)
    }

    /// The values of the handles the owner hands over, in [`Slot::HANDED`]'s order (the pipe
    /// folder's is not among them).
    pub fn raw(&self) -> [usize; HANDED_COUNT] {
        let all = self.handles.each_ref();
        std::array::from_fn(|i| all[i].as_raw_handle().expose_provenance())
    }

    /// The `--lowbox-handles` text of [`NamespaceHandles::raw`], for a child that inherits the
    /// handles (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST` with [`NamespaceHandles::inheritable`]).
    pub fn to_arg(&self) -> String {
        format_handle_arg(&self.raw())
    }

    /// Makes every handed-over handle ([`Slot::HANDED`]) inheritable until the returned guard
    /// drops, for a launch that lists them in `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`. The caller
    /// holds `clauseo_process`'s spawn lock (`lock`) from before this call until the guard is
    /// gone, so no other start in this process inherits them meanwhile. The guard turns the
    /// flag off again on every path, and a failure part way turns off the ones already on.
    pub fn inheritable<'a>(&'a self, lock: &'a SpawnLock) -> Result<Inheritable<'a>, Error> {
        let mut guard = Inheritable {
            handles: self,
            on: 0,
            _lock: lock,
        };
        for h in self.handles().into_iter().take(HANDED_COUNT) {
            set_flag(h, true)?;
            guard.on += 1;
        }
        Ok(guard)
    }

    /// Turns `HANDLE_FLAG_INHERIT` on or off on every handle.
    fn set_inherit(&self, inheritable: bool) -> Result<(), Error> {
        for h in self.handles() {
            set_flag(h, inheritable)?;
        }
        Ok(())
    }

    /// Copies every handed-over handle ([`Slot::HANDED`]) into `process` (opened with
    /// `PROCESS_DUP_HANDLE`), not inheritable, with only the access the guard needs
    /// (`READ_CONTROL` and query: it checks each object and hands it to the token call), and
    /// returns the values there, in [`Slot::HANDED`]'s order.
    /// For a guard started without inheritance (`CreateProcessWithLogonW`), which then gets the
    /// values some other way. A failure part way leaves the earlier copies in `process`.
    pub fn duplicate_into(
        &self,
        process: BorrowedHandle<'_>,
    ) -> Result<[usize; HANDED_COUNT], Error> {
        let target = HANDLE(process.as_raw_handle());
        let mut out = [0_usize; HANDED_COUNT];
        for ((h, slot), out) in self.handles().into_iter().zip(Slot::HANDED).zip(&mut out) {
            let access = match slot.kind() {
                ObjectKind::Directory => DIRECTORY_GUARD,
                ObjectKind::SymbolicLink => LINK_OPEN,
                ObjectKind::File => PIPES_OPEN,
            };
            let mut copy = HANDLE::default();
            // SAFETY: `h` is open in this process and `target` is a process handle the caller
            // holds; the copy lives in that process, so nothing here owns or closes it.
            unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    h,
                    target,
                    &raw mut copy,
                    access,
                    false,
                    DUPLICATE_HANDLE_OPTIONS(0),
                )
            }
            .map_err(|e| win32("DuplicateHandle", &e))?;
            *out = copy.0.expose_provenance();
        }
        Ok(out)
    }

    /// The package SID.
    pub(super) fn package_sid(&self) -> &Sid {
        &self.package
    }

    /// The handles as `HANDLE`s, valid while `self` lives.
    pub(super) fn handles(&self) -> [HANDLE; HANDLE_COUNT] {
        self.handles.each_ref().map(|h| HANDLE(h.as_raw_handle()))
    }
}

/// Opens the object directory `name` with `access`.
fn open_directory(name: &ObjectName, access: u32) -> Result<OwnedHandle, Error> {
    let attributes = name.attributes(None);
    let mut out = HANDLE::default();
    // SAFETY: `attributes` points at `name`, which outlives the call; `out` gets the handle.
    let status = unsafe { NtOpenDirectoryObject(&raw mut out, access, &raw const attributes) };
    nt_ok("NtOpenDirectoryObject", status)?;
    Ok(owned(out))
}

/// Opens the symbolic link `name` for the token call.
fn open_link(name: &ObjectName) -> Result<OwnedHandle, Error> {
    let attributes = name.attributes(None);
    let mut out = HANDLE::default();
    // SAFETY: as in `open_directory`.
    let status =
        unsafe { NtOpenSymbolicLinkObject(&raw mut out, LINK_OPEN, &raw const attributes) };
    nt_ok("NtOpenSymbolicLinkObject", status)?;
    Ok(owned(out))
}

/// Makes the directory `name` with `descriptor`, shadowing `shadow`.
fn create_directory(
    name: &ObjectName,
    descriptor: &Descriptor,
    shadow: &OwnedHandle,
) -> Result<OwnedHandle, Error> {
    let attributes = name.attributes(Some(descriptor));
    let mut out = HANDLE::default();
    // SAFETY: `attributes` points at `name` and `descriptor`, and `shadow` is open; all outlive
    // the call. `out` gets the handle.
    let status = unsafe {
        NtCreateDirectoryObjectEx(
            &raw mut out,
            DIRECTORY_ALL_ACCESS,
            &raw const attributes,
            HANDLE(shadow.as_raw_handle()),
            0,
        )
    };
    nt_ok("NtCreateDirectoryObjectEx", status)?;
    Ok(owned(out))
}

/// Makes the link `name` with `descriptor`, pointing at `target`.
fn create_link(
    name: &ObjectName,
    descriptor: &Descriptor,
    target: &ObjectName,
) -> Result<OwnedHandle, Error> {
    let attributes = name.attributes(Some(descriptor));
    let mut out = HANDLE::default();
    // SAFETY: `attributes` points at `name` and `descriptor`, `target` is a valid counted
    // string; all outlive the call. `out` gets the handle.
    let status = unsafe {
        NtCreateSymbolicLinkObject(
            &raw mut out,
            LINK_ALL_ACCESS,
            &raw const attributes,
            target.as_ptr(),
        )
    };
    nt_ok("NtCreateSymbolicLinkObject", status)?;
    Ok(owned(out))
}

/// Turns `HANDLE_FLAG_INHERIT` on or off on `h`.
fn set_flag(h: HANDLE, on: bool) -> Result<(), Error> {
    let flags = if on {
        HANDLE_FLAG_INHERIT
    } else {
        HANDLE_FLAGS(0)
    };
    // SAFETY: `h` is an open handle of this process (the caller's); only its flag changes.
    unsafe { SetHandleInformation(h, HANDLE_FLAG_INHERIT.0, flags) }
        .map_err(|e| win32("SetHandleInformation", &e))
}

/// Makes the pipe folder `name` with `descriptor`, which must not exist yet: `FILE_OPEN_IF`, as
/// the recipe did, then the folder is refused unless the call reports `FILE_CREATED`, its
/// owner is `owner` and its ACEs read back as `descriptor`'s. A folder someone made first,
/// with a descriptor of theirs, is never used.
fn new_pipe_folder(
    name: &ObjectName,
    descriptor: &Descriptor,
    owner: &Sid,
) -> Result<OwnedHandle, Error> {
    let (folder, information) = pipe_folder(name, Some(descriptor), PIPES_OWNER, FILE_OPEN_IF)?;
    if information != FILE_CREATED {
        return Err(Error::Namespace {
            reason: "the pipe folder was already there",
        });
    }
    let found = token::owner_of(HANDLE(folder.as_raw_handle())).map_err(|code| Error::Win32 {
        call: "GetSecurityInfo (the pipe folder's owner)",
        code,
    })?;
    if found != owner.as_str() {
        return Err(Error::Namespace {
            reason: "the new pipe folder is not owned by this process's user",
        });
    }
    let dacl = token::dacl_of(HANDLE(folder.as_raw_handle())).map_err(|code| Error::Win32 {
        call: "GetSecurityInfo (the pipe folder's DACL)",
        code,
    })?;
    if !handles::same_aces(&dacl, &descriptor.dacl_sddl()?) {
        return Err(Error::Namespace {
            reason: "the new pipe folder's DACL is not the one it was made with",
        });
    }
    Ok(folder)
}

/// Opens (and with `FILE_OPEN_IF` and a descriptor, makes) the pipe folder `name`, following no
/// object-manager link (`OBJ_DONT_REPARSE`); the second value is `IO_STATUS_BLOCK.Information`
/// (`FILE_CREATED`, `FILE_OPENED`, …).
fn pipe_folder(
    name: &ObjectName,
    descriptor: Option<&Descriptor>,
    access: u32,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
) -> Result<(OwnedHandle, usize), Error> {
    // `\Device\NamedPipe\…` holds no object-manager link, so none may be followed.
    let attributes = name.attributes_no_reparse(descriptor);
    let mut out = HANDLE::default();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: `attributes` points at `name` and `descriptor`, which outlive the call; the
    // status block and `out` are written by it. No EA buffer.
    let status = unsafe {
        NtCreateFile(
            &raw mut out,
            FILE_ACCESS_RIGHTS(access),
            &raw const attributes,
            &raw mut status_block,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            disposition,
            FILE_DIRECTORY_FILE,
            None,
            0,
        )
    };
    nt_ok("NtCreateFile", status)?;
    Ok((owned(out), status_block.Information))
}

/// The guard's own handle to the pipe folder of `expected`'s package in its session: opened by
/// name with [`PIPES_OPEN`] and `FILE_OPEN` (never made), then checked through that handle by
/// [`handles::check_pipe_folder`] against the owner and the ACEs of F1 for `expected.owner`,
/// this process's user (the browser account) and the package.
fn open_own_pipe_folder(expected: &Expected<'_>) -> Result<OwnedHandle, Error> {
    let name = ObjectName::new(&layout::object_name(
        Slot::Pipes,
        expected.session,
        expected.package,
    ))?;
    let (folder, _opened) =
        pipe_folder(&name, None, PIPES_OPEN, FILE_OPEN).map_err(|e| match e {
            Error::Nt { status, .. } => Error::Nt {
                call: "NtCreateFile (the package's pipe folder, by name)",
                status,
            },
            other => other,
        })?;
    let browser = token::current_user()?;
    let wanted = Descriptor::from_sddl(&layout::pipe_folder_sddl(
        expected.owner,
        &browser,
        expected.package,
    ))?
    .dacl_sddl()?;
    let raw = HANDLE(folder.as_raw_handle());
    handles::check_pipe_folder(token::owner_of(raw), token::dacl_of(raw), expected, &wanted)?;
    Ok(folder)
}

/// Takes ownership of a handle a create or open call just returned.
fn owned(handle: HANDLE) -> OwnedHandle {
    // SAFETY: the call succeeded, so `handle` is open, and nothing else holds it.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}
