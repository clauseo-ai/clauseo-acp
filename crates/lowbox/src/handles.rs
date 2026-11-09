//! The namespace's six handles: their slots, the `--lowbox-handles` text that carries five of
//! their values to the guard, the checks a value passes before the guard uses it, and the checks
//! the guard makes of the pipe folder it opens itself. The checks run through [`Probe`] and plain
//! strings, so they are tested off Windows; `windows::probe` is the real probe.
//!
//! The pipe folder is never handed over: `NtQueryObject` names a handle to an NPFS prefix just
//! `\Device\NamedPipe` (seen on Server 2025, r842), so a handed-over value could not be told
//! from any other NPFS handle. The guard opens it by its name instead (`crate::windows`).

use crate::error::{Error, HandleProblem};
use crate::layout;
use crate::package::Sid;

/// How many handles the namespace has: one per [`Slot`].
pub const HANDLE_COUNT: usize = 6;

/// How many of them the owner hands to the guard: every slot in [`Slot::HANDED`].
pub const HANDED_COUNT: usize = 5;

/// The most hex digits a handle value takes (64 bits).
const MAX_DIGITS: usize = 16;

/// One of the namespace's objects, in the order `NtCreateLowBoxToken` gets their handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// `\Sessions\<n>\AppContainerNamedObjects\<package SID>`, shadowing the session's
    /// `BaseNamedObjects`.
    Directory,
    /// `…\RPC Control`, shadowing `\RPC Control`.
    RpcControl,
    /// `…\Global`, shadowing `\BaseNamedObjects`.
    Global,
    /// `…\Local`, a link to the directory.
    Local,
    /// `…\Session`, a link to the directory.
    Session,
    /// `\Device\NamedPipe\Sessions\<n>\AppContainerNamedObjects\<package SID>`, the package's
    /// named-pipe folder.
    Pipes,
}

/// The kernel object type a slot holds, as `NtQueryObject(ObjectTypeInformation)` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// An object directory.
    Directory,
    /// An object symbolic link.
    SymbolicLink,
    /// A file object: the named-pipe folder is opened with `NtCreateFile`.
    File,
}

impl Slot {
    /// Every slot, in handle order.
    pub const ALL: [Slot; HANDLE_COUNT] = [
        Slot::Directory,
        Slot::RpcControl,
        Slot::Global,
        Slot::Local,
        Slot::Session,
        Slot::Pipes,
    ];

    /// The slots whose handles the owner hands over, in handle order: all but [`Slot::Pipes`],
    /// which the guard opens by name.
    pub const HANDED: [Slot; HANDED_COUNT] = [
        Slot::Directory,
        Slot::RpcControl,
        Slot::Global,
        Slot::Local,
        Slot::Session,
    ];

    /// A short name for errors.
    pub fn name(self) -> &'static str {
        match self {
            Slot::Directory => "the package directory",
            Slot::RpcControl => "RPC Control",
            Slot::Global => "Global",
            Slot::Local => "Local",
            Slot::Session => "Session",
            Slot::Pipes => "the pipe folder",
        }
    }

    /// The object type the slot holds.
    pub fn kind(self) -> ObjectKind {
        match self {
            Slot::Directory | Slot::RpcControl | Slot::Global => ObjectKind::Directory,
            Slot::Local | Slot::Session => ObjectKind::SymbolicLink,
            Slot::Pipes => ObjectKind::File,
        }
    }

    /// The object's name below the package directory (`""` for the directory itself), or
    /// `None` for the pipe folder, which lives under `\Device\NamedPipe` (`crate::layout`).
    pub(crate) fn below_directory(self) -> Option<&'static str> {
        match self {
            Slot::Directory => Some(""),
            Slot::RpcControl => Some(r"\RPC Control"),
            Slot::Global => Some(r"\Global"),
            Slot::Local => Some(r"\Local"),
            Slot::Session => Some(r"\Session"),
            Slot::Pipes => None,
        }
    }
}

impl ObjectKind {
    /// The kernel's name for the type.
    pub fn type_name(self) -> &'static str {
        match self {
            ObjectKind::Directory => "Directory",
            ObjectKind::SymbolicLink => "SymbolicLink",
            ObjectKind::File => "File",
        }
    }
}

/// The handle values from `--lowbox-handles`: exactly [`HANDED_COUNT`] distinct, non-zero
/// `0x`-prefixed hex values joined by commas, in [`Slot::HANDED`]'s order (`0x1a4,0x1a8,…`).
pub fn parse_handle_arg(text: &str) -> Result<Vec<usize>, Error> {
    let values = text
        .split(',')
        .map(|part| {
            let digits = part.strip_prefix("0x").ok_or(Error::HandleList {
                reason: "a value has no 0x",
            })?;
            if digits.is_empty()
                || digits.len() > MAX_DIGITS
                || !digits.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(Error::HandleList {
                    reason: "a value is not 1 to 16 hex digits",
                });
            }
            usize::from_str_radix(digits, 16).map_err(|_too_big| Error::HandleList {
                reason: "a value does not fit a handle",
            })
        })
        .collect::<Result<Vec<usize>, Error>>()?;
    check_values(&values)?;
    Ok(values)
}

/// The `--lowbox-handles` text for `values`.
pub(crate) fn format_handle_arg(values: &[usize]) -> String {
    values
        .iter()
        .map(|v| format!("{v:#x}"))
        .collect::<Vec<String>>()
        .join(",")
}

/// The count, non-zero and distinct checks, which need no OS.
fn check_values(values: &[usize]) -> Result<(), Error> {
    if values.len() != HANDED_COUNT {
        return Err(Error::HandleList {
            reason: "not exactly five values",
        });
    }
    if values.contains(&0) {
        return Err(Error::HandleList {
            reason: "a value is zero",
        });
    }
    for (i, v) in values.iter().enumerate() {
        if values[..i].contains(v) {
            return Err(Error::HandleList {
                reason: "a value is given twice",
            });
        }
    }
    Ok(())
}

/// The handles a launch passes on by value (`LowBoxCommand::inherit_handle`): none null or a
/// pseudo handle (`GetCurrentProcess` and the like, negative as a signed value), none twice.
pub(crate) fn check_inherit_list(values: &[usize]) -> Result<(), Error> {
    for (i, v) in values.iter().enumerate() {
        if *v == 0 || v.cast_signed() < 0 {
            return Err(Error::InheritList {
                reason: "a null or pseudo handle",
            });
        }
        if values[..i].contains(v) {
            return Err(Error::InheritList {
                reason: "a handle listed twice",
            });
        }
    }
    Ok(())
}

/// What the checks ask of a handle value in this process.
pub(crate) trait Probe {
    /// This process's standard input, output and error handle values (0 for one it lacks).
    fn std_handles(&self) -> [usize; 3];
    /// `Ok` when `value` is an open handle, else `GetLastError()`.
    fn check_open(&self, value: usize) -> Result<(), u32>;
    /// The object's type name, else the NTSTATUS.
    fn type_name(&self, value: usize) -> Result<String, i32>;
    /// The object's full name, else the NTSTATUS.
    fn object_name(&self, value: usize) -> Result<String, i32>;
    /// The `S-1-…` text of the object's owner, else the Win32 error.
    fn owner(&self, value: usize) -> Result<String, u32>;
}

/// What each handle must be: the objects of the package with SID `package` in session
/// `session` (the guard's own), owned by `owner` (the user who built the namespace).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Expected<'a> {
    /// The package SID.
    pub(crate) package: &'a Sid,
    /// The session the namespace must be in: the caller's.
    pub(crate) session: u32,
    /// The objects' owner.
    pub(crate) owner: &'a Sid,
}

/// Each of `values` checked for its slot in [`Slot::HANDED`]: not one of this process's
/// standard handles, an open handle, of the slot's type, named exactly as the slot's object in
/// `expected`'s namespace (`crate::layout::object_name`, letters in either case) and owned by
/// `expected.owner`. Fails closed at the first value that does not pass.
pub(crate) fn check(
    probe: &impl Probe,
    values: &[usize],
    expected: &Expected<'_>,
) -> Result<(), Error> {
    check_values(values)?;
    let standard = probe.std_handles();
    for (slot, &value) in Slot::HANDED.iter().copied().zip(values) {
        let fail = |problem| Error::Handle { slot, problem };
        if standard.contains(&value) {
            return Err(fail(HandleProblem::StandardHandle));
        }
        probe
            .check_open(value)
            .map_err(|code| fail(HandleProblem::NotOpen { code }))?;
        let found = probe
            .type_name(value)
            .map_err(|status| fail(HandleProblem::TypeUnreadable { status }))?;
        if found != slot.kind().type_name() {
            return Err(fail(HandleProblem::WrongType { found }));
        }
        let name = probe
            .object_name(value)
            .map_err(|status| fail(HandleProblem::NameUnreadable { status }))?;
        let wanted = layout::object_name(slot, expected.session, expected.package);
        if !name.eq_ignore_ascii_case(&wanted) {
            return Err(fail(HandleProblem::WrongName {
                found: redact(&name, expected.package),
                expected: redact(&wanted, expected.package),
            }));
        }
        let owner = probe
            .owner(value)
            .map_err(|code| fail(HandleProblem::OwnerUnreadable { code }))?;
        if owner != expected.owner.as_str() {
            return Err(fail(HandleProblem::WrongOwner));
        }
    }
    Ok(())
}

/// The pipe folder the guard opened by name, checked through that handle: owned by
/// `expected.owner` (`found_owner`, the owner's `S-1-…` text or the Win32 error) and with
/// exactly the ACEs of `expected_dacl` (`found_dacl`, the DACL as SDDL or the Win32 error). Both
/// texts come from the same conversion (`ConvertSecurityDescriptorToStringSecurityDescriptorW`),
/// so SID aliases and the access masks are written alike; the DACL's control flags (`P`, `AI`,
/// `AR`) are not compared, since they grant nothing.
pub(crate) fn check_pipe_folder(
    found_owner: Result<String, u32>,
    found_dacl: Result<String, u32>,
    expected: &Expected<'_>,
    expected_dacl: &str,
) -> Result<(), Error> {
    let fail = |problem| Error::Handle {
        slot: Slot::Pipes,
        problem,
    };
    let owner = found_owner.map_err(|code| fail(HandleProblem::OwnerUnreadable { code }))?;
    if owner != expected.owner.as_str() {
        return Err(fail(HandleProblem::WrongOwner));
    }
    let dacl = found_dacl.map_err(|code| fail(HandleProblem::DaclUnreadable { code }))?;
    if same_aces(&dacl, expected_dacl) {
        Ok(())
    } else {
        Err(fail(HandleProblem::WrongDacl))
    }
}

/// Whether the SDDL DACLs `found` and `wanted` hold exactly the same ACEs, in the same order
/// (their control flags aside); `false` when either is no DACL with ACEs (see [`aces`]).
pub(crate) fn same_aces(found: &str, wanted: &str) -> bool {
    matches!((aces(found), aces(wanted)), (Some(f), Some(w)) if f == w)
}

/// The ACEs of the SDDL DACL `dacl` (`D:`, optional control flags, then the `(…)` ACEs), or
/// `None` when it is no DACL with at least one ACE (no `D:`, `D:NO_ACCESS_CONTROL`, an empty
/// DACL, or text after the DACL such as an `S:` part).
fn aces(dacl: &str) -> Option<&str> {
    let rest = dacl.strip_prefix("D:")?;
    let start = rest.find('(')?;
    let flags = &rest[..start];
    let list = &rest[start..];
    let flags_ok =
        flags.is_empty() || ["P", "AI", "AR", "PAI", "PAR", "ARAI", "PARAI"].contains(&flags);
    (flags_ok && list.ends_with(')') && !list.contains(")S:")).then_some(list)
}

/// The start of an app package SID's text.
const PACKAGE_SID_PREFIX: &str = "s-1-15-2-";

/// `name` with `package` (letters in either case) written `<package SID>` and any other app
/// package SID (`S-1-15-2-…`) written `<another package SID>`, so a refusal shows the name's
/// form without logging a package SID.
pub(crate) fn redact(name: &str, package: &Sid) -> String {
    // ASCII lowering keeps every byte offset, so positions found in `lower` index `name`.
    let lower = name.to_ascii_lowercase();
    let mut out = String::with_capacity(name.len());
    let mut rest = 0;
    while let Some(found) = lower[rest..].find(PACKAGE_SID_PREFIX) {
        let start = rest + found;
        let body = start + PACKAGE_SID_PREFIX.len();
        let digits = lower[body..]
            .find(|c: char| !(c.is_ascii_digit() || c == '-'))
            .map_or(lower.len(), |n| body + n);
        // A SID's text ends in a digit; a trailing '-' is the name's, not the SID's.
        let end = body + lower[body..digits].trim_end_matches('-').len();
        out.push_str(&name[rest..start]);
        out.push_str(if name[start..end].eq_ignore_ascii_case(package.as_str()) {
            "<package SID>"
        } else {
            "<another package SID>"
        });
        rest = end;
    }
    out.push_str(&name[rest..]);
    out
}

#[cfg(test)]
mod tests;
