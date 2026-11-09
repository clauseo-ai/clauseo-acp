//! The namespace's six handles: their slots, the `--lowbox-handles` text that carries their
//! values to the guard, and the checks a value passes before the guard uses it. The checks run
//! through [`Probe`], so they are tested off Windows; `windows::probe` is the real one.

use crate::error::{Error, HandleProblem};

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
    /// `None` for the pipe folder, which lives under `\Device\NamedPipe`.
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
    /// `Ok` when `value` is an open handle, else `GetLastError()`.
    fn check_open(&self, value: usize) -> Result<(), u32>;
    /// The object's type name, else the NTSTATUS.
    fn type_name(&self, value: usize) -> Result<String, i32>;
    /// The object's full name, else the NTSTATUS. Never asked of the pipe folder.
    fn object_name(&self, value: usize) -> Result<String, i32>;
}

/// Each of `values` checked for its slot: an open handle, of the slot's type, and (all but the
/// pipe folder) named as the slot's object in the namespace of the package whose SID text is
/// `package_sid`: `\Sessions\<n>\AppContainerNamedObjects\<package SID>` and below. Fails
/// closed at the first value that does not pass.
pub(crate) fn check(probe: &impl Probe, values: &[usize], package_sid: &str) -> Result<(), Error> {
    check_values(values)?;
    let mut directory: Option<String> = None;
    for (slot, &value) in Slot::ALL.iter().copied().zip(values) {
        let fail = |problem| Error::Handle { slot, problem };
        probe
            .check_open(value)
            .map_err(|code| fail(HandleProblem::NotOpen { code }))?;
        let found = probe
            .type_name(value)
            .map_err(|status| fail(HandleProblem::TypeUnreadable { status }))?;
        if found != slot.kind().type_name() {
            return Err(fail(HandleProblem::WrongType { found }));
        }
        let Some(below) = slot.below_directory() else {
            continue;
        };
        let name = probe
            .object_name(value)
            .map_err(|status| fail(HandleProblem::NameUnreadable { status }))?;
        let matches = match &directory {
            None => is_package_directory(&name, package_sid),
            Some(dir) => {
                name.len() == dir.len() + below.len()
                    && name
                        .get(..dir.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(dir))
                    && name
                        .get(dir.len()..)
                        .is_some_and(|tail| tail.eq_ignore_ascii_case(below))
            }
        };
        if !matches {
            return Err(fail(HandleProblem::WrongName));
        }
        if directory.is_none() {
            directory = Some(name);
        }
    }
    Ok(())
}

/// Whether `name` is `\Sessions\<n>\AppContainerNamedObjects\<package_sid>`, letters in either
/// case.
fn is_package_directory(name: &str, package_sid: &str) -> bool {
    let parts: Vec<&str> = name.split('\\').collect();
    matches!(
        parts.as_slice(),
        ["", sessions, n, container, sid]
            if sessions.eq_ignore_ascii_case("Sessions")
                && !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && container.eq_ignore_ascii_case("AppContainerNamedObjects")
                && sid.eq_ignore_ascii_case(package_sid)
    )
}

#[cfg(test)]
mod tests;
