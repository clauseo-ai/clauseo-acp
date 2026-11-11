//! Running stetdoc for a tracked-changes job: on the calling (matter) thread, through a
//! passthrough launcher with a clean environment ([`env`] adds back what Windows needs), within
//! [`READ_LIMIT`] or [`EDIT_LIMIT`]; and its answer mapped to the `word.*` codes.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_profile::skills::stetdoc::{Ran, RunFailure, RunOptions, STDOUT_MAX, run_for};
use clauseo_protocol::ErrorCode;
use serde::Deserialize;

use crate::clock::{Clock, ProcessTimer};
use crate::error::Error;

/// How long a read (`check`, `view`) may take.
pub(crate) const READ_LIMIT: Duration = Duration::from_secs(10);

/// How long an edit may take.
pub(crate) const EDIT_LIMIT: Duration = Duration::from_secs(30);

/// The most stdout a read keeps: `view --revisions --full --all` of a heavily redlined
/// document runs past the skill runs' 1 MiB. The parse keeps the first `MAX_REVISIONS`.
pub(crate) const READ_STDOUT_MAX: u64 = 16 * 1024 * 1024;

/// The Windows directory's variable, which system DLLs read.
const SYSTEM_ROOT: &str = "SystemRoot";

/// stetdoc's refusal on stdout under `--json`.
#[derive(Deserialize)]
struct Refusal {
    error: RefusalBody,
}

#[derive(Deserialize)]
struct RefusalBody {
    code: String,
}

/// One stetdoc program, the clock its deadlines run on and the job's private folder.
pub(super) struct Runner<'a> {
    program: &'a Path,
    clock: &'a Arc<dyn Clock>,
    work: &'a Path,
}

impl<'a> Runner<'a> {
    pub(super) const fn new(program: &'a Path, clock: &'a Arc<dyn Clock>, work: &'a Path) -> Self {
        Self {
            program,
            clock,
            work,
        }
    }

    /// stetdoc's JSON answer to `args` (each run with `--json`): an edit within [`EDIT_LIMIT`]
    /// and 1 MiB of stdout, a read within [`READ_LIMIT`] and [`READ_STDOUT_MAX`]. Blocks the
    /// calling thread until it ends.
    pub(super) fn json(&self, args: &[OsString], edit: bool) -> Result<serde_json::Value, Error> {
        let launcher = PassthroughLauncher::new(SpawnOptions::new().clear_env().end_with_parent());
        let timer = ProcessTimer(Arc::clone(self.clock));
        let (limit, stdout_max) = if edit {
            (EDIT_LIMIT, STDOUT_MAX)
        } else {
            (READ_LIMIT, READ_STDOUT_MAX)
        };
        let options = RunOptions {
            limit,
            stdout_max,
            env: env(
                cfg!(windows),
                |name| std::env::var_os(name),
                crate::agent::loads::case_law::windows_directory,
                self.work,
            ),
        };
        let ran =
            futures::executor::block_on(run_for(&launcher, &timer, self.program, args, &options));
        answer(ran)
    }
}

/// The variables stetdoc gets on top of a cleared environment: none off Windows (`windows`
/// false). On Windows `SystemRoot`, as every child the host starts gets it (system DLLs read
/// it), from `lookup` (the host's environment) or else `windows_dir`, left out when neither
/// has it; and `TEMP` and `TMP` set to `work`, the job's private folder, so whatever stetdoc
/// writes there goes with the folder.
pub(super) fn env(
    windows: bool,
    lookup: impl Fn(&str) -> Option<OsString>,
    windows_dir: impl Fn() -> Option<OsString>,
    work: &Path,
) -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::new();
    if !windows {
        return env;
    }
    if let Some(root) = lookup(SYSTEM_ROOT).or_else(windows_dir) {
        env.insert(OsString::from(SYSTEM_ROOT), root);
    }
    for name in ["TEMP", "TMP"] {
        env.insert(OsString::from(name), work.as_os_str().to_owned());
    }
    env
}

/// The JSON stetdoc printed when it exited 0; otherwise its failure as a `word.*` code.
pub(super) fn answer(ran: Result<Ran, RunFailure>) -> Result<serde_json::Value, Error> {
    let ran = ran.map_err(|failure| {
        // The kind only: a launch failure's reason may name a path (ADR 0047).
        let kind = match failure {
            RunFailure::Launch(_) => "launch",
            RunFailure::TimedOut => "timed_out",
            RunFailure::TooMuchOutput { .. } => "too_much_output",
            RunFailure::Read(_) => "read",
        };
        tracing::warn!(kind, "stetdoc did not finish");
        match failure {
            RunFailure::Launch(_) => Error::refused(
                ErrorCode::WordStetdocMissing,
                "stetdoc could not be started",
            ),
            RunFailure::TimedOut => {
                Error::refused(ErrorCode::WordEditFailed, "stetdoc did not finish in time")
            }
            RunFailure::TooMuchOutput { .. } | RunFailure::Read(_) => {
                Error::refused(ErrorCode::WordEditFailed, "stetdoc's answer did not read")
            }
        }
    })?;
    if ran.code == Some(0) {
        return serde_json::from_slice(&ran.stdout).map_err(|err| {
            // The class only: serde's message can quote document values (ADR 0047).
            tracing::warn!(class = ?err.classify(), "stetdoc's JSON did not read");
            Error::refused(ErrorCode::WordEditFailed, "stetdoc's answer did not read")
        });
    }
    let code = serde_json::from_slice::<Refusal>(&ran.stdout)
        .map(|refusal| refusal.error.code)
        .ok();
    // The exit and stetdoc's refusal code only: its stderr can quote the document (ADR 0047).
    tracing::warn!(exit = ?ran.code, code = ?code, "stetdoc refused");
    let Some(code) = code else {
        return Err(Error::refused(
            ErrorCode::WordEditFailed,
            format!("stetdoc failed with exit {:?}", ran.code),
        ));
    };
    Err(Error::refused(
        refusal_code(&code),
        format!("stetdoc refused: {code}"),
    ))
}

/// The code for stetdoc's refusal `code` (`stetdoc capabilities`).
pub(super) fn refusal_code(code: &str) -> ErrorCode {
    match code {
        // A change id that is gone, or a file that moved under stetdoc.
        "not_found" | "stale_ref" | "stale_target" | "stale_files" => ErrorCode::WordRevisionsStale,
        // A file stetdoc cannot read as a Word document (exit 6).
        "malformed_input" | "limit_exceeded" | "corrupt_complement" | "unsupported_version" => {
            ErrorCode::WordNotDocx
        }
        _ => ErrorCode::WordEditFailed,
    }
}
