//! The launch spec: what to start, where, and with which variables, checked once when it is built.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::Error;

/// A program to start, with its arguments, the variables to set on top of the inherited
/// environment, and its working folder. Built only by [`LaunchSpec::new`], so every spec has
/// passed its checks.
///
/// Its `Debug` names each variable but shows no value: the env may carry an API key
/// (decision record 0048, ruling 5), which never reaches a log, an error or a `Debug`. The
/// extra secrets it hides ([`LaunchSpec::with_redacted`]) show only as a count.
#[derive(Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    program: PathBuf,
    args: Vec<OsString>,
    env: BTreeMap<OsString, OsString>,
    cwd: PathBuf,
    redacted: Redacted,
    priority_class: Option<u32>,
}

/// Windows' `BELOW_NORMAL_PRIORITY_CLASS` process creation flag.
pub const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Secrets the stderr tail hides beyond the env's: `Debug` prints only how many.
#[derive(Clone, Default, PartialEq, Eq)]
struct Redacted(Vec<String>);

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{} redacted]", self.0.len())
    }
}

impl LaunchSpec {
    /// Checks and builds a spec. `program` and `cwd` must be absolute and `cwd` must be a folder,
    /// so nothing is looked up on `PATH` or in the current folder, which differ by OS. A program
    /// whose name ends in `.cmd` or `.bat`, in any case and with any trailing dots or spaces
    /// (which Windows drops), is refused on every OS: Windows runs batch files through `cmd.exe`,
    /// a shell. A working folder that is not a folder is [`Error::NoWorkingFolder`]; each other
    /// failure is [`Error::InvalidSpec`].
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
            return Err(Error::NoWorkingFolder);
        }
        Ok(LaunchSpec {
            program,
            args,
            env,
            cwd,
            redacted: Redacted::default(),
            priority_class: None,
        })
    }

    /// The same spec, with `secrets` added to what the agent's stderr tail hides, whatever
    /// variable (if any) holds them: a secret the agent reads from a file, such as the case-law
    /// relay's token (R24). The same length rule as the env's secrets applies. A launcher that
    /// builds a new spec around this one carries them over ([`Self::redacted`]).
    #[must_use]
    pub fn with_redacted(mut self, secrets: impl IntoIterator<Item = String>) -> LaunchSpec {
        self.redacted.0.extend(secrets);
        self
    }

    /// The secrets [`Self::with_redacted`] added.
    pub fn redacted(&self) -> &[String] {
        &self.redacted.0
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

    /// The same spec started in Windows' priority class `class` (a `*_PRIORITY_CLASS` creation
    /// flag, such as [`BELOW_NORMAL_PRIORITY_CLASS`] for a background run nobody waits on). No
    /// other OS changes anything.
    #[must_use]
    pub fn with_priority_class(mut self, class: u32) -> LaunchSpec {
        self.priority_class = Some(class);
        self
    }

    /// The priority class [`Self::with_priority_class`] set, if any.
    pub fn priority_class(&self) -> Option<u32> {
        self.priority_class
    }

    /// The process creation flags the spec adds on Windows to its launcher's: its priority
    /// class, else none.
    pub fn creation_flags(&self) -> u32 {
        self.priority_class.unwrap_or(0)
    }
}

impl std::fmt::Debug for LaunchSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchSpec")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("env", &RedactedEnv(&self.env))
            .field("cwd", &self.cwd)
            .field("redacted", &self.redacted)
            .field("priority_class", &self.priority_class)
            .finish()
    }
}

/// An env's names, each value shown as `[redacted]`.
struct RedactedEnv<'a>(&'a BTreeMap<OsString, OsString>);

impl std::fmt::Debug for RedactedEnv<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.0.keys().map(|name| (name, format_args!("[redacted]"))))
            .finish()
    }
}

/// An [`Error::InvalidSpec`] saying `which` `what`. The path is not in it: it can hold the
/// user's name, and the text reaches the lawyer's screen as a failure's detail.
fn invalid(which: &str, _path: &Path, what: &str) -> Error {
    Error::InvalidSpec(format!("{which} {what}"))
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
