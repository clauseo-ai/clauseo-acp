//! The launch spec: what to start, where, and with which variables, checked once when it is built.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::Error;

/// A program to start, with its arguments, the variables to set on top of the inherited
/// environment, and its working folder. Built only by [`LaunchSpec::new`], so every spec has
/// passed its checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    program: PathBuf,
    args: Vec<OsString>,
    env: BTreeMap<OsString, OsString>,
    cwd: PathBuf,
}

impl LaunchSpec {
    /// Checks and builds a spec. `program` and `cwd` must be absolute and `cwd` must be a folder,
    /// so nothing is looked up on `PATH` or in the current folder, which differ by OS. A program
    /// whose name ends in `.cmd` or `.bat`, in any case and with any trailing dots or spaces
    /// (which Windows drops), is refused on every OS: Windows runs batch files through `cmd.exe`,
    /// a shell. Each failure is [`Error::InvalidSpec`].
    pub fn new(
        program: PathBuf,
        args: Vec<OsString>,
        env: BTreeMap<OsString, OsString>,
        cwd: PathBuf,
    ) -> Result<LaunchSpec, Error> {
        if !program.is_absolute() {
            return Err(invalid("the program", &program, "is not an absolute path"));
        }
        if is_batch_file(program.as_os_str()) {
            return Err(invalid("the program", &program, "is a batch file"));
        }
        if !cwd.is_absolute() {
            return Err(invalid(
                "the working folder",
                &cwd,
                "is not an absolute path",
            ));
        }
        if !cwd.is_dir() {
            return Err(invalid("the working folder", &cwd, "is not a folder"));
        }
        Ok(LaunchSpec {
            program,
            args,
            env,
            cwd,
        })
    }

    /// The program, an absolute path.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The arguments, without the program.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// The variables set on top of the inherited environment; each one overrides the inherited
    /// value of the same name.
    pub fn env(&self) -> &BTreeMap<OsString, OsString> {
        &self.env
    }

    /// The working folder, an absolute path to a folder.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
}

/// An [`Error::InvalidSpec`] saying `which` (`path`) `what`.
fn invalid(which: &str, path: &Path, what: &str) -> Error {
    Error::InvalidSpec(format!("{which} {} {what}", path.display()))
}

/// Whether `program` names a batch file: it ends in `.cmd` or `.bat`, in any case, once trailing
/// dots and spaces are dropped.
fn is_batch_file(program: &OsStr) -> bool {
    let name = program.as_encoded_bytes();
    let end = name
        .iter()
        .rposition(|&byte| byte != b'.' && byte != b' ')
        .map_or(0, |last| last + 1);
    let name = name.get(..end).unwrap_or_default();
    let ext = name
        .len()
        .checked_sub(4)
        .and_then(|start| name.get(start..));
    ext.is_some_and(|ext| ext.eq_ignore_ascii_case(b".cmd") || ext.eq_ignore_ascii_case(b".bat"))
}

#[cfg(test)]
mod tests;
