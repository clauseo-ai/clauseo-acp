//! The namespace's six handles: their slots, the `--lowbox-handles` text that carries their
//! values to the guard, and the checks a value passes before the guard uses it. The checks run
//! through [`Probe`], so they are tested off Windows; `windows::probe` is the real one.

use crate::error::{Error, HandleProblem};
use crate::layout;
use crate::package::Sid;

/// How many handles the namespace has: one per [`Slot`].
pub const HANDLE_COUNT: usize = 6;

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

/// The handle values from `--lowbox-handles`: exactly [`HANDLE_COUNT`] distinct, non-zero
/// `0x`-prefixed hex values joined by commas, in [`Slot::ALL`]'s order (`0x1a4,0x1a8,…`).
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
    if values.len() != HANDLE_COUNT {
        return Err(Error::HandleList {
            reason: "not exactly six values",
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

/// Each of `values` checked for its slot: not one of this process's standard handles, an open
/// handle, of the slot's type, named exactly as the slot's object in `expected`'s namespace
/// (`crate::layout::object_name`, letters in either case) and owned by `expected.owner`. Fails
/// closed at the first value that does not pass.
pub(crate) fn check(
    probe: &impl Probe,
    values: &[usize],
    expected: &Expected<'_>,
) -> Result<(), Error> {
    check_values(values)?;
    let standard = probe.std_handles();
    for (slot, &value) in Slot::ALL.iter().copied().zip(values) {
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
            return Err(fail(HandleProblem::WrongName));
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

#[cfg(test)]
mod tests;
