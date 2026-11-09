//! The crate's one error type. Each failed Windows call carries its NTSTATUS or Win32 code.

use std::fmt;

use crate::handles::Slot;

/// Why a launch, a namespace or an argument failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The package name is not 1 to 64 ASCII letters, digits, dots and dashes.
    PackageName {
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A SID string is not of the form `S-1-<authority>-<sub>…`.
    SidText,
    /// An argument or the program cannot be written on a Windows command line.
    Argument {
        /// The argument's index: 0 is the program.
        index: usize,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// An environment entry cannot go in an environment block.
    Environment {
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A browser folder cannot be granted to the package.
    Grant {
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The handle list from the command line is not exactly [`crate::HANDLE_COUNT`] distinct hex
    /// values.
    HandleList {
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The handles a launch passes on are not distinct, open, real handles.
    InheritList {
        /// What is wrong with them.
        reason: &'static str,
    },
    /// A handle value given for a slot of the namespace failed its check.
    Handle {
        /// The slot it was given for.
        slot: Slot,
        /// What the check found.
        problem: HandleProblem,
    },
    /// A native call returned a failing NTSTATUS.
    Nt {
        /// The call.
        call: &'static str,
        /// Its NTSTATUS, as the bits Windows returns.
        status: i32,
    },
    /// A Win32 call failed.
    Win32 {
        /// The call.
        call: &'static str,
        /// `GetLastError()`, or the code the call returned.
        code: u32,
    },
    /// The owner refused to build or hand over a namespace: another session than its own, or an
    /// object that was there before it, or not as it made it.
    Namespace {
        /// What is wrong.
        reason: &'static str,
    },
    /// A token is not what a `LowBox` launch needs: the guard's own token (elevated, an
    /// administrator, already an `AppContainer`) or the new `LowBox` token (not this package,
    /// not Low, a capability or a privilege left).
    Token {
        /// What is wrong.
        reason: &'static str,
    },
}

/// What [`crate::NamespaceHandles::from_inherited`] found wrong with one handle value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandleProblem {
    /// The value is not an open handle in this process (`GetHandleInformation` failed).
    NotOpen {
        /// `GetLastError()`.
        code: u32,
    },
    /// Its object type could not be read (`NtQueryObject(ObjectTypeInformation)` failed).
    TypeUnreadable {
        /// The NTSTATUS.
        status: i32,
    },
    /// The object is of another type than the slot's.
    WrongType {
        /// The type name the kernel reported, such as `Event`.
        found: String,
    },
    /// The object's name could not be read (`NtQueryObject(ObjectNameInformation)` failed).
    NameUnreadable {
        /// The NTSTATUS.
        status: i32,
    },
    /// The object is not the slot's object in this package's namespace. Both names are kept
    /// with every app package SID in them written as a placeholder (`crate::handles::redact`),
    /// since a package SID is never logged; the form of the name is what a refusal shows.
    WrongName {
        /// The name the kernel reported, redacted.
        found: String,
        /// The name the slot's object has, redacted.
        expected: String,
    },
    /// The value is one of this process's standard handles, which it never takes over.
    StandardHandle,
    /// The object's owner could not be read (`GetSecurityInfo` failed).
    OwnerUnreadable {
        /// The Win32 error.
        code: u32,
    },
    /// The object is not owned by the user who built the namespace.
    WrongOwner,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PackageName { reason } => write!(f, "bad package name: {reason}"),
            Error::SidText => f.write_str("bad SID string"),
            Error::Argument { index, reason } => write!(f, "argument {index}: {reason}"),
            Error::Environment { reason } => write!(f, "environment: {reason}"),
            Error::Grant { reason } => write!(f, "grant: {reason}"),
            Error::HandleList { reason } => write!(f, "handle list: {reason}"),
            Error::InheritList { reason } => write!(f, "inherited handles: {reason}"),
            Error::Handle { slot, problem } => write!(f, "handle for {}: {problem}", slot.name()),
            Error::Nt { call, status } => {
                // Hex of the bits, as Windows writes an NTSTATUS (0xC0000022).
                write!(
                    f,
                    "{call} failed with NTSTATUS {:#010X}",
                    status.cast_unsigned()
                )
            }
            Error::Win32 { call, code } => write!(f, "{call} failed with Win32 error {code}"),
            Error::Namespace { reason } => write!(f, "namespace: {reason}"),
            Error::Token { reason } => write!(f, "token: {reason}"),
        }
    }
}

impl fmt::Display for HandleProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandleProblem::NotOpen { code } => write!(f, "not an open handle (error {code})"),
            HandleProblem::TypeUnreadable { status } => {
                write!(
                    f,
                    "type unreadable (NTSTATUS {:#010X})",
                    status.cast_unsigned()
                )
            }
            HandleProblem::WrongType { found } => write!(f, "wrong object type {found}"),
            HandleProblem::NameUnreadable { status } => {
                write!(
                    f,
                    "name unreadable (NTSTATUS {:#010X})",
                    status.cast_unsigned()
                )
            }
            HandleProblem::WrongName { found, expected } => write!(
                f,
                "not this package's object (the kernel names it \"{found}\", expected \"{expected}\")"
            ),
            HandleProblem::StandardHandle => f.write_str("one of this process's standard handles"),
            HandleProblem::OwnerUnreadable { code } => {
                write!(f, "owner unreadable (error {code})")
            }
            HandleProblem::WrongOwner => f.write_str("not owned by the namespace's builder"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ntstatus_reads_as_windows_writes_it() {
        let error = Error::Nt {
            call: "NtCreateLowBoxToken",
            status: 0xC000_0022_u32.cast_signed(),
        };
        assert_eq!(
            error.to_string(),
            "NtCreateLowBoxToken failed with NTSTATUS 0xC0000022"
        );
    }

    #[test]
    fn a_handle_error_names_its_slot() {
        let error = Error::Handle {
            slot: Slot::Session,
            problem: HandleProblem::WrongType {
                found: "Event".to_owned(),
            },
        };
        assert_eq!(
            error.to_string(),
            "handle for Session: wrong object type Event"
        );
    }

    #[test]
    fn a_wrong_name_shows_both_names() {
        let error = Error::Handle {
            slot: Slot::Pipes,
            problem: HandleProblem::WrongName {
                found: r"\Device\NamedPipe\".to_owned(),
                expected: r"\Device\NamedPipe\Sessions\2\AppContainerNamedObjects\<package SID>"
                    .to_owned(),
            },
        };
        assert_eq!(
            error.to_string(),
            r#"handle for the pipe folder: not this package's object (the kernel names it "\Device\NamedPipe\", expected "\Device\NamedPipe\Sessions\2\AppContainerNamedObjects\<package SID>")"#
        );
    }
}
