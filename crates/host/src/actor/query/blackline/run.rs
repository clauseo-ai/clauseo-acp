//! The two stetdoc runs of a Word blackline, off the actor: `stetdoc compare` writes the
//! redline of `old.docx` and `new.docx`, then `stetdoc view --all --json` prints it, which
//! `parse.rs` reads.
//!
//! It answers: how stetdoc is started (through the host's launcher, as every child is: the
//! absolute program, no shell, an environment of its own (`stetdoc_env`: nothing off Windows;
//! `SystemRoot`, `windir`, and `TEMP` and `TMP` in the blackline's folder on Windows), the
//! blackline's folder as its working folder, its tree killed when its handle drops); how long
//! each run may take ([`COMPARE_LIMIT`], [`VIEW_LIMIT`], on the host clock); how much it may print
//! ([`STDOUT_MAX`]); and what counts as a failure ([`Failed`]): a launch that fails, an exit
//! other than 0, the deadline, stdout past the cap or not UTF-8, a view that does not read, and
//! a view with a warning.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, Timer};
use clauseo_protocol::WordBlackline;
use futures::AsyncReadExt as _;
use futures::future::{Either, join, select};

use super::parse::read_view;
use crate::config::Stetdoc;
use crate::matter_worker::{NEW, OLD};

/// How long `stetdoc compare` may take.
pub(crate) const COMPARE_LIMIT: Duration = Duration::from_secs(60);

/// How long `stetdoc view` may take.
pub(crate) const VIEW_LIMIT: Duration = Duration::from_secs(30);

/// The most bytes of stdout one run may print (8 MiB); past it stetdoc is killed and the
/// blackline fails.
pub(crate) const STDOUT_MAX: u64 = 8 * 1024 * 1024;

/// The redline `compare` writes and `view` reads.
pub(crate) const REDLINE: &str = "redline.docx";

/// Why a blackline failed, for the log: which run and what went wrong. It holds no document
/// text: stetdoc's stderr is left out, and of its stdout only an error's code is kept.
#[derive(Debug)]
pub(crate) struct Failed {
    /// `compare` or `view`.
    pub(crate) step: &'static str,
    /// What went wrong.
    pub(crate) why: String,
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "stetdoc {}: {}", self.step, self.why)
    }
}

/// The blackline of `<dir>/old.docx` and `<dir>/new.docx`: `compare` writes
/// `<dir>/redline.docx`, then `view` prints it.
pub(crate) async fn blackline<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Stetdoc,
    dir: &Path,
) -> Result<WordBlackline, Failed> {
    let path = |name: &str| dir.join(name).into_os_string();
    let compare = vec![
        "compare".into(),
        "--json".into(),
        "--author".into(),
        OsString::from(&stetdoc.author),
        "-o".into(),
        path(REDLINE),
        path(OLD),
        path(NEW),
    ];
    run(
        launcher,
        timer,
        stetdoc,
        dir,
        "compare",
        compare,
        COMPARE_LIMIT,
    )
    .await?;
    let view = vec![
        "view".into(),
        "--all".into(),
        "--json".into(),
        path(REDLINE),
    ];
    let printed = run(launcher, timer, stetdoc, dir, "view", view, VIEW_LIMIT).await?;
    read_view(&printed).map_err(|failure| Failed {
        step: "view",
        why: failure.to_string(),
    })
}

/// The value of `name` in `env`, the name matched in any case (Windows' rule).
fn lookup(env: &[(OsString, OsString)], name: &str) -> Option<OsString> {
    env.iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

/// stetdoc's whole environment: nothing off Windows. On Windows (`windows`), the minimum the
/// profile's other spawned tools get (`case_law::windows_env`): `SystemRoot` and `windir`, each
/// read through `lookup` (the host's environment) and, when that has none, from `windows_dir`
/// (the Windows directory), since Windows programs may not start without them; then `TEMP` and
/// `TMP` set to `dir`, the blackline's own folder, so whatever stetdoc writes there goes with it.
fn stetdoc_env(
    windows: bool,
    lookup: impl Fn(&str) -> Option<OsString>,
    windows_dir: impl Fn() -> Option<OsString>,
    dir: &Path,
) -> BTreeMap<OsString, OsString> {
    let mut env = BTreeMap::new();
    if !windows {
        return env;
    }
    for name in ["SystemRoot", "windir"] {
        if let Some(value) = lookup(name).or_else(&windows_dir) {
            env.insert(OsString::from(name), value);
        } else {
            tracing::warn!(name, "the host has no Windows directory for stetdoc");
        }
    }
    for name in ["TEMP", "TMP"] {
        env.insert(OsString::from(name), dir.as_os_str().to_owned());
    }
    env
}

/// stetdoc's stdout for `args`, run in `dir` within `limit`.
async fn run<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Stetdoc,
    dir: &Path,
    step: &'static str,
    args: Vec<OsString>,
    limit: Duration,
) -> Result<String, Failed> {
    let failed = |why: String| Failed { step, why };
    let host_env: Vec<(OsString, OsString)> = if cfg!(windows) {
        std::env::vars_os().collect()
    } else {
        Vec::new()
    };
    let env = stetdoc_env(
        cfg!(windows),
        |name| lookup(&host_env, name),
        crate::agent::loads::case_law::windows_directory,
        dir,
    );
    let spec = LaunchSpec::new(stetdoc.program.clone(), args, env, dir.to_owned())
        .map_err(|err| failed(format!("it could not be started: {err}")))?;
    let Launched {
        stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher
        .launch(&spec)
        .map_err(|err| failed(format!("it could not be started: {err}")))?;
    drop(stdin);
    let mut out = Vec::new();
    let ran = {
        let work = async {
            let mut limited = stdout.take(STDOUT_MAX + 1);
            let mut drain = pin!(stderr.drain());
            let (read, drained) = {
                let reading = pin!(limited.read_to_end(&mut out));
                match select(reading, drain.as_mut()).await {
                    Either::Left((read, _)) => (read, false),
                    Either::Right((_, reading)) => (reading.await, true),
                }
            };
            if read.is_ok() && u64::try_from(out.len()).unwrap_or(u64::MAX) > STDOUT_MAX {
                return None;
            }
            let exit = if drained {
                handle.exited().await
            } else {
                join(drain, handle.exited()).await.1
            };
            Some((read, exit))
        };
        match select(pin!(work), pin!(timer.after(limit))).await {
            Either::Left((ran, _)) => Some(ran),
            Either::Right(((), _)) => None,
        }
    };
    // Dropping the handle kills the tree, if anything of it is left: a run past the cap or the
    // deadline is still writing.
    drop(handle);
    let (read, exit) = ran
        .ok_or_else(|| failed(format!("it took longer than {} s", limit.as_secs())))?
        .ok_or_else(|| failed("its stdout ran past 8 MiB; it was killed".to_owned()))?;
    read.map_err(|err| failed(format!("its stdout failed: {err}")))?;
    if exit.code() != Some(0) {
        return Err(failed(format!(
            "it exited with {:?} (error code {})",
            exit.code(),
            error_code(&out)
        )));
    }
    String::from_utf8(out).map_err(|_bad| failed("its stdout is not UTF-8".to_owned()))
}

/// The `error.code` of stetdoc's JSON error on `stdout`, such as `malformed_input`, or `none`:
/// a short word of stetdoc's, never the document's text.
fn error_code(stdout: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(stdout)
        .ok()
        .and_then(|json| {
            json.pointer("/error/code")
                .and_then(serde_json::Value::as_str)
                .filter(|code| code.len() <= 64 && code.bytes().all(|b| b.is_ascii_graphic()))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "none".to_owned())
}

#[cfg(test)]
mod tests;
