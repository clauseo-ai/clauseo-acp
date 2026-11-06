//! `<data_dir>\sandbox\setup-result.json` (S15 `PR4h`, D-B4): what the setup exe tells the app
//! after `--set-up` or `--reclaim`, since `ShellExecuteExW` gives it no output, only an exit
//! code. `{ exit, step, cause }`: the exit code again, and for a failure the step and the
//! cause in words firm IT can act on. The exe writes it as the owner (`Owner::run_as`), so the
//! file and its DACL are the owner's; `UserSetup` removes it before each run and reads it after.
//!
//! [`outcome`] turns the exit code and the file into what `SandboxSetup::run` returns. Pure
//! apart from the file's read and write: tested on every OS.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::outcome::{SetupExit, SetupFailure, SetupReport};
use crate::SetupError;
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::write;

/// The file's name, in `<data_dir>\sandbox\`.
pub const RESULT_FILE: &str = "setup-result.json";

/// What one run of the exe ended with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupResult {
    /// The exit code ([`SetupExit::as_code`]).
    pub exit: u8,
    /// Where it failed ([`Step`](super::Step)'s words); none when it didn't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// Why; none when it didn't fail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

impl SetupResult {
    /// The result of a set-up the engine ran.
    pub fn of(outcome: &Result<SetupReport, SetupFailure>) -> SetupResult {
        match outcome {
            Ok(_) => SetupResult {
                exit: SetupExit::Done.as_code(),
                step: None,
                cause: None,
            },
            Err(failure) => SetupResult {
                exit: failure.exit.as_code(),
                step: Some(failure.step.to_string()),
                cause: Some(failure.cause.clone()),
            },
        }
    }

    /// A failure before or around the engine: `exit` at `step`, because of `cause`.
    pub fn failed(exit: SetupExit, step: &str, cause: impl Into<String>) -> SetupResult {
        SetupResult {
            exit: exit.as_code(),
            step: Some(step.to_owned()),
            cause: Some(cause.into()),
        }
    }
}

/// The file's path under `data_dir`.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(SANDBOX_DIR).join(RESULT_FILE)
}

/// Writes `result` atomically, as `marker::write` writes `setup.json`: the folder made if
/// missing, the old file checked first and the new one after (`write::check_owned_file`). The
/// exe calls it acting as the owner.
pub fn write(data_dir: &Path, result: &SetupResult) -> io::Result<()> {
    let file = path(data_dir);
    let mut bytes = serde_json::to_vec_pretty(result).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if let Some(folder) = file.parent() {
        write::make_sandbox_dir(folder)?;
    }
    write::check_owned_file(&file)?;
    write::write_atomic(&file, &bytes)?;
    write::check_owned_file(&file)
}

/// Reads the file: `Ok(None)` when it is missing; why when it can't be read or parsed, or is
/// a link or has a second hard link (`write::check_owned_file`).
pub fn read(data_dir: &Path) -> Result<Option<SetupResult>, String> {
    let file = path(data_dir);
    write::check_owned_file(&file).map_err(|error| error.to_string())?;
    match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// Removes the file, so a run that writes none isn't read as the last one's. Missing is fine.
#[cfg(any(windows, test))]
pub fn remove(data_dir: &Path) -> io::Result<()> {
    write::remove_owned_file(&path(data_dir))
}

/// What a run of the exe means for the app, from its exit `code` and the result file as
/// [`read`] gave it after the run (`None` for `--remove`, which writes none). 0 is done,
/// whatever the file says. Any other code fails; the file's step and cause say why when it
/// matches the code, else the code's own words do, naming the code.
pub fn outcome(
    code: u32,
    result: Option<&Result<Option<SetupResult>, String>>,
) -> Result<(), SetupError> {
    let exit = SetupExit::from_code(code);
    if exit == Some(SetupExit::Done) {
        return Ok(());
    }
    let told = match result {
        Some(Ok(Some(told))) if u32::from(told.exit) == code => {
            told.cause.as_ref().map(|cause| match &told.step {
                Some(step) => format!("{step}: {cause}"),
                None => cause.clone(),
            })
        }
        _ => None,
    };
    let unread = match result {
        Some(Err(why)) => format!("; its result can't be read ({why})"),
        Some(Ok(_)) if told.is_none() => "; it left no result".to_owned(),
        _ => String::new(),
    };
    let words = match exit {
        Some(SetupExit::OtherOwner) => {
            "another Windows user set Clauseo's protection up on this computer".to_owned()
        }
        Some(SetupExit::LockHeld) => "another set-up is running on this computer".to_owned(),
        Some(SetupExit::Usage) => {
            format!("Clauseo's set-up program was started wrongly (exit {code})")
        }
        Some(SetupExit::RemovedPartly) => {
            format!("Clauseo's set-up program removed only part of the protection (exit {code})")
        }
        Some(SetupExit::Failed | SetupExit::Done) | None => {
            format!("Clauseo's set-up program failed (exit {code})")
        }
    };
    Err(SetupError::Failed(match told {
        Some(told) => format!("{words}: {told}"),
        None => format!("{words}{unread}"),
    }))
}

#[cfg(test)]
mod tests;
