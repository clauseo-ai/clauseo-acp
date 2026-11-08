//! How the setup exe explains a failure that would otherwise leave no trace: the exe sets up no
//! log output, so `tracing` lines from `--adopt` and `--remove --uninstall` went nowhere.
//!
//! A failed `--adopt` step ([`AdoptFailure`]) and an admin copy `--remove --uninstall` couldn't
//! take ([`CopyLeft`]) each give one line for the Application Event Log (source "Clauseo
//! Sandbox", `refresh::event_log`): the step's name and the error's code, never a path or a SID.
//! `--adopt`'s also becomes `setup-result.json` once the lawyer is confirmed
//! ([`AdoptFailure::result`]), with the cause's words and the code. Pure: tested on every OS.

use std::fmt;

use super::machine::MachineError;
use super::outcome::SetupExit;
use super::result_file::SetupResult;
use crate::windows::refresh::SystemTask;

/// An error's code, as Windows gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// A Win32 error code (`GetLastError`, a `NET_API_STATUS`, a registry call's status).
    Win32(u32),
    /// An HRESULT (COM, the Task Scheduler).
    Hresult(u32),
}

impl Code {
    /// The kind's name in `setup-result.json`: `win32` or `hresult`.
    pub fn kind(self) -> &'static str {
        match self {
            Code::Win32(_) => "win32",
            Code::Hresult(_) => "hresult",
        }
    }

    /// The number itself.
    pub fn value(self) -> u32 {
        match self {
            Code::Win32(code) | Code::Hresult(code) => code,
        }
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Code::Win32(code) => write!(f, "Win32 error {code}"),
            Code::Hresult(code) => write!(f, "HRESULT {code:#010x}"),
        }
    }
}

/// Words for an error's code, or for none.
fn code_words(code: Option<Code>) -> String {
    code.map_or_else(
        || "no Windows error code".to_owned(),
        |code| code.to_string(),
    )
}

/// Why a step failed: the words, for the log and the result file only (they may name a path),
/// and the code, when Windows gave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cause {
    /// What failed, in words.
    pub text: String,
    /// The error's code.
    pub code: Option<Code>,
}

impl Cause {
    /// `text`, with the code `code`.
    pub fn with(text: impl Into<String>, code: Code) -> Cause {
        Cause {
            text: text.into(),
            code: Some(code),
        }
    }
}

impl From<String> for Cause {
    fn from(text: String) -> Cause {
        Cause { text, code: None }
    }
}

impl From<&str> for Cause {
    fn from(text: &str) -> Cause {
        Cause::from(text.to_owned())
    }
}

impl From<MachineError> for Cause {
    fn from(error: MachineError) -> Cause {
        let code = match &error {
            MachineError::Os { code, .. } => Some(Code::Win32(*code)),
            MachineError::LockHeld
            | MachineError::Unsupported(_)
            | MachineError::Other(_)
            | MachineError::ProfileChanged(_) => None,
        };
        Cause {
            text: error.to_string(),
            code,
        }
    }
}

impl From<std::io::Error> for Cause {
    /// A raw OS error is a Win32 code on Windows; one wrapping a `windows` crate error gives
    /// its HRESULT.
    fn from(error: std::io::Error) -> Cause {
        let code = error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
            .map(Code::Win32);
        #[cfg(windows)]
        let code = code.or_else(|| {
            error
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<windows::core::Error>())
                .map(|inner| Code::Hresult(inner.code().0.cast_unsigned()))
        });
        Cause {
            text: error.to_string(),
            code,
        }
    }
}

/// The `--adopt` step that failed, as `setup-result.json` and the Event Log name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdoptStep {
    /// The DLL search couldn't be kept to System32 (`exe::main`, before the mode is known).
    DllSearch,
    /// Step 1: this process's token.
    Token,
    /// Step 2: `FOLDERID_ProgramFiles`.
    ProgramFiles,
    /// Step 2: the running image's final path.
    RunningImage,
    /// Step 3: the lawyer the unelevated installer named (`Owner::of_installer`).
    Lawyer,
    /// Step 3: the set-up lock.
    Lock,
    /// Step 3: the machine's record.
    Record,
    /// Step 4: the lawyer's profile.
    Profile,
    /// Step 5: the admin folder's lock-down.
    LockDown,
    /// Step 7: the admin copy's hash.
    Hash,
    /// Step 6: a SYSTEM task's registration.
    Task(SystemTask),
    /// Step 7: the record's admin-copy values.
    Values,
    /// Step 8: the set-up found no confirmed lawyer to run as.
    SetUpUser,
}

impl AdoptStep {
    /// The step's name: stable, `snake_case`, path- and SID-free.
    pub fn name(self) -> &'static str {
        match self {
            AdoptStep::DllSearch => "dll_search",
            AdoptStep::Token => "token",
            AdoptStep::ProgramFiles => "program_files",
            AdoptStep::RunningImage => "running_image",
            AdoptStep::Lawyer => "lawyer",
            AdoptStep::Lock => "lock",
            AdoptStep::Record => "record",
            AdoptStep::Profile => "profile",
            AdoptStep::LockDown => "lock_down",
            AdoptStep::Hash => "hash",
            AdoptStep::Task(SystemTask::Refresh) => "refresh_task",
            AdoptStep::Task(SystemTask::Stop) => "stop_task",
            AdoptStep::Task(SystemTask::WslDeny) => "wsl_deny_task",
            AdoptStep::Task(SystemTask::WslUpgrade) => "wsl_upgrade_task",
            AdoptStep::Values => "admin_values",
            AdoptStep::SetUpUser => "set_up_user",
        }
    }
}

/// A `--adopt` step that failed, which exits [`SetupExit::Failed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptFailure {
    /// Where.
    pub step: AdoptStep,
    /// Why.
    pub cause: Cause,
}

impl AdoptFailure {
    /// The Event Log's line: `--adopt failed (exit 3) at step <name>: <code>`.
    pub fn line(&self) -> String {
        let exit = SetupExit::Failed.as_code();
        let code = code_words(self.cause.code);
        format!(
            "--adopt failed (exit {exit}) at step {}: {code}",
            self.step.name()
        )
    }

    /// `setup-result.json`'s result: exit 3, the step, the cause's words and its code.
    pub fn result(&self) -> SetupResult {
        let mut result =
            SetupResult::failed(SetupExit::Failed, self.step.name(), self.cause.text.clone());
        result.code = self.cause.code.map(Code::value);
        result.kind = self.cause.code.map(|code| code.kind().to_owned());
        result
    }
}

/// The admin copy, or part of it, that `--remove --uninstall` couldn't take: the teardown step
/// (`refresh::Teardown`'s, or `record` when the record couldn't be read) and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyLeft {
    /// The step's name.
    pub step: &'static str,
    /// Why.
    pub cause: Cause,
}

impl CopyLeft {
    /// The Event Log's line: `--remove --uninstall left the admin copy (exit 10) at step
    /// <name>: <code>`.
    pub fn line(&self) -> String {
        let exit = SetupExit::AdminCopyLeft.as_code();
        let code = code_words(self.cause.code);
        format!(
            "--remove --uninstall left the admin copy (exit {exit}) at step {}: {code}",
            self.step
        )
    }
}

/// A finished removal's exit once the admin copy's removal (`taken`) is known: `Removed`, or
/// [`SetupExit::AdminCopyLeft`] with its line given to `log` (the Event Log).
pub fn removal_exit(taken: Result<(), CopyLeft>, log: &mut dyn FnMut(&str)) -> SetupExit {
    match taken {
        Ok(()) => SetupExit::Removed,
        Err(left) => {
            log(&left.line());
            SetupExit::AdminCopyLeft
        }
    }
}

#[cfg(test)]
mod tests;
