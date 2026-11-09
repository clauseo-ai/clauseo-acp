//! The package's pipe folder (an NPFS prefix): made by the owner, opened by name by the guard,
//! and checked through the handle each gets. Every decision is made here over [`PipeFs`], so
//! the squatted and tampered cases are tested off Windows; `crate::windows::namespace` has the
//! real one (`NtCreateFile`, `GetSecurityInfo`).
//!
//! The folder is never handed over: `NtQueryObject` names any NPFS prefix handle just
//! `\Device\NamedPipe` (seen on Server 2025, r842), so a handed-over value could not be told from
//! any other NPFS handle.

use super::{Expected, Slot};
use crate::error::{Error, HandleProblem};
use crate::layout;
use crate::package::Sid;

/// `FILE_CREATED`: `IO_STATUS_BLOCK.Information` when `FILE_OPEN_IF` made the file.
pub(crate) const FILE_CREATED: usize = 2;

/// A descriptor's DACL and mandatory label, each as SDDL written by
/// `ConvertSecurityDescriptorToStringSecurityDescriptorW` (`D:…` and `S:…`), so SID aliases and
/// access masks read alike whichever descriptor they come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Security {
    /// The DACL (`DACL_SECURITY_INFORMATION`).
    pub(crate) dacl: String,
    /// The mandatory label (`LABEL_SECURITY_INFORMATION`).
    pub(crate) label: String,
}

/// What making and opening a pipe folder asks of the system.
pub(crate) trait PipeFs {
    /// An open folder; dropping it closes it.
    type Folder;
    /// Opens the folder `name`, making it with the descriptor `sddl` when it is not there
    /// (`FILE_OPEN_IF`); the folder and `IO_STATUS_BLOCK.Information` ([`FILE_CREATED`] when made).
    fn create(&self, name: &str, sddl: &str) -> Result<(Self::Folder, usize), Error>;
    /// Opens the existing folder `name` (`FILE_OPEN`, never made) with `READ_CONTROL` and
    /// `SYNCHRONIZE` only.
    fn open(&self, name: &str) -> Result<Self::Folder, Error>;
    /// The `S-1-…` text of the folder's owner, else the Win32 error.
    fn owner(&self, folder: &Self::Folder) -> Result<String, u32>;
    /// The folder's DACL and label, read through `folder`, else the Win32 error.
    fn security(&self, folder: &Self::Folder) -> Result<Security, u32>;
    /// The DACL and label of the descriptor `sddl`, written as [`PipeFs::security`] writes them.
    fn security_of(&self, sddl: &str) -> Result<Security, Error>;
}

/// The owner's side: makes the pipe folder of `package` in session `session` with
/// [`layout::pipe_folder_sddl`] for `owner` (this process's user), `browser` and `package`.
/// Refused unless the call made it ([`FILE_CREATED`]: a folder someone made first, with a
/// descriptor of theirs, is never used), its owner is `owner`, and its ACEs and label read back
/// as the descriptor's.
pub(crate) fn make<F: PipeFs>(
    fs: &F,
    session: u32,
    owner: &Sid,
    browser: &Sid,
    package: &Sid,
) -> Result<F::Folder, Error> {
    let name = layout::object_name(Slot::Pipes, session, package);
    let sddl = layout::pipe_folder_sddl(owner, browser, package);
    let (folder, information) = fs.create(&name, &sddl)?;
    if information != FILE_CREATED {
        return Err(Error::Namespace {
            reason: "the pipe folder was already there",
        });
    }
    let found = fs.owner(&folder).map_err(|code| Error::Win32 {
        call: "GetSecurityInfo (the pipe folder's owner)",
        code,
    })?;
    if found != owner.as_str() {
        return Err(Error::Namespace {
            reason: "the new pipe folder is not owned by this process's user",
        });
    }
    let read = fs.security(&folder).map_err(|code| Error::Win32 {
        call: "GetSecurityInfo (the pipe folder's DACL and label)",
        code,
    })?;
    let wanted = fs.security_of(&sddl)?;
    if !same_aces(&read.dacl, &wanted.dacl, "D:") || !same_aces(&read.label, &wanted.label, "S:") {
        return Err(Error::Namespace {
            reason: "the new pipe folder's DACL or label is not the one it was made with",
        });
    }
    Ok(folder)
}

/// The guard's side: opens the pipe folder of `expected`'s package in its session by that name
/// alone (never made, no other name tried), then checks through that folder that it is the one
/// the owner made ([`check`]) against [`layout::pipe_folder_sddl`] for `expected.owner`,
/// `browser` (this process's user) and the package. The folder is dropped (closed) on a refusal.
pub(crate) fn open<F: PipeFs>(
    fs: &F,
    expected: &Expected<'_>,
    browser: &Sid,
) -> Result<F::Folder, Error> {
    let name = layout::object_name(Slot::Pipes, expected.session, expected.package);
    let folder = fs.open(&name)?;
    let wanted = fs.security_of(&layout::pipe_folder_sddl(
        expected.owner,
        browser,
        expected.package,
    ))?;
    check(fs.owner(&folder), fs.security(&folder), expected, &wanted)?;
    Ok(folder)
}

/// The pipe folder the guard opened, checked through its handle: owned by `expected.owner`
/// (`found_owner`, else the Win32 error), with exactly `wanted`'s ACEs in `wanted`'s order and
/// exactly `wanted`'s label ACE (`found`, else the Win32 error). The control flags of the DACL
/// and the SACL (`P`, `AI`, `AR`) are not compared, since they grant nothing. Each refusal is a
/// [`HandleProblem`] for [`Slot::Pipes`].
pub(crate) fn check(
    found_owner: Result<String, u32>,
    found: Result<Security, u32>,
    expected: &Expected<'_>,
    wanted: &Security,
) -> Result<(), Error> {
    let fail = |problem| Error::Handle {
        slot: Slot::Pipes,
        problem,
    };
    let owner = found_owner.map_err(|code| fail(HandleProblem::OwnerUnreadable { code }))?;
    if owner != expected.owner.as_str() {
        return Err(fail(HandleProblem::WrongOwner));
    }
    let found = found.map_err(|code| fail(HandleProblem::SecurityUnreadable { code }))?;
    if !same_aces(&found.dacl, &wanted.dacl, "D:") {
        return Err(fail(HandleProblem::WrongDacl));
    }
    if !same_aces(&found.label, &wanted.label, "S:") {
        return Err(fail(HandleProblem::WrongLabel));
    }
    Ok(())
}

/// Whether the SDDL ACLs `found` and `wanted` (each `part`, `D:` or `S:`, then optional control
/// flags, then ACEs) hold exactly the same ACEs in the same order; `false` when either has no
/// ACE list (see [`aces`]).
fn same_aces(found: &str, wanted: &str, part: &str) -> bool {
    matches!((aces(found, part), aces(wanted, part)), (Some(f), Some(w)) if f == w)
}

/// The ACEs of the SDDL ACL `acl`: `part` (`D:` or `S:`), optional control flags, then one or
/// more `(…)` ACEs and nothing after them. `None` for anything else: another part, no ACL
/// (`NO_ACCESS_CONTROL`), an empty one, unknown flags, or text after the ACEs (another part).
fn aces<'a>(acl: &'a str, part: &str) -> Option<&'a str> {
    let rest = acl.strip_prefix(part)?;
    let start = rest.find('(')?;
    let (flags, list) = rest.split_at(start);
    let flags_ok =
        flags.is_empty() || ["P", "AI", "AR", "PAI", "PAR", "ARAI", "PARAI"].contains(&flags);
    // Every ACE closes before the next opens, and the last closes the text.
    let whole = list.ends_with(')')
        && list
            .match_indices(')')
            .all(|(i, _)| matches!(list.as_bytes().get(i + 1), None | Some(b'(')));
    (flags_ok && whole).then_some(list)
}

#[cfg(test)]
mod tests;
