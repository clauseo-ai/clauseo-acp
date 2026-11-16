//! The stetdoc runs of a Word blackline, off the actor: `stetdoc compare` writes the redline of
//! `old.docx` and `new.docx` and says what it could not compare (`parse/receipt.rs`), then
//! `stetdoc view --all --json` prints it, which `parse.rs` reads; then the views of its notes
//! and headers (`stories.rs`), the runs that check it (`cross.rs`: `stetdoc check` on the
//! redline and a view of each version), and Clauseo's own comparison of the versions' table
//! formatting (`tables.rs`), none of which fails the blackline: a run that fails leaves it with
//! a notice.
//!
//! It answers: how stetdoc is started (through the host's launcher, as every child is: the
//! absolute program, no shell, an environment of its own (`stetdoc_env`: nothing off Windows;
//! `SystemRoot`, `windir`, and `TEMP` and `TMP` in the blackline's folder on Windows), the
//! blackline's folder as its working folder, its tree killed when its handle drops); how long
//! each run may take ([`COMPARE_LIMIT`], [`VIEW_LIMIT`], on the host clock); how much it may print
//! ([`STDOUT_MAX`]); what counts as a failure ([`Failed`]): a launch that fails, an exit
//! other than 0, the deadline, stdout past the cap or not UTF-8, and a view with nothing in it
//! that can be shown (a view that reads in part, or warns, is a blackline with notices); that
//! the blackline may differ when the two files differ but it shows no change and names none
//! ([`note_a_silent_difference`]); and which code a failure gets ([`refusal_code`]): stetdoc's
//! own refusals (a table inside a table, a compare it refused, a version it can't read, a size
//! limit) their own, the rest `blackline.failed`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, Timer};
use clauseo_protocol::{BlacklineNotice, ErrorCode, ParagraphChange, WordBlackline};
use futures::AsyncReadExt as _;
use futures::future::{Either, join, select};

use super::parse::receipt::read_compare;
use super::parse::story::add_parts;
use super::parse::{Read, ViewFailure, order, read_view};
use super::tables::{self, TablesCompared};
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

/// Why a blackline failed: which run and what went wrong, for the log, and the code the caller
/// gets. It holds no document text: stetdoc's stderr is left out, and of its stdout only an
/// error's code is kept.
#[derive(Debug)]
pub(crate) struct Failed {
    /// `compare` or `view`.
    pub(crate) step: &'static str,
    /// What went wrong.
    pub(crate) why: String,
    /// What the caller is told: `blackline.failed` unless stetdoc refused with a code of its own
    /// ([`refusal_code`]).
    pub(crate) code: ErrorCode,
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
) -> Result<Read, Failed> {
    let path = |name: &str| dir.join(name).into_os_string();
    let compare = vec![
        "compare".into(),
        "--json".into(),
        author_arg(&stetdoc.author),
        "-o".into(),
        path(REDLINE),
        path(OLD),
        path(NEW),
    ];
    let compared = run(
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
    let mut read = read_view(&printed).map_err(|failure| Failed {
        step: "view",
        why: failure.to_string(),
        code: match failure {
            ViewFailure::FormatUnknown => ErrorCode::BlacklineFormatUnknown,
            ViewFailure::NotAView(_) | ViewFailure::NothingRead { .. } => {
                ErrorCode::BlacklineFailed
            }
        },
    })?;
    let said = read_compare(&compared);
    read.blackline.notices.extend(said.notices);
    read.warnings.extend(said.codes);
    order(&mut read.blackline.notices);
    // The story views and the check run at once: the check's list is applied once the parts'
    // ids are counted.
    let (stories, listed) = join(
        stories::view_stories(launcher, timer, stetdoc, dir),
        cross::list_changes(launcher, timer, stetdoc, dir),
    )
    .await;
    add_parts(&mut read, stories);
    // Two copies the host just wrote; one that can't be read counts as different.
    let sides = match (std::fs::read(dir.join(OLD)), std::fs::read(dir.join(NEW))) {
        (Ok(old), Ok(new)) => Some((old, new)),
        _ => None,
    };
    let differ = sides.as_ref().is_none_or(|(old, new)| old != new);
    cross::cross_check(launcher, timer, stetdoc, dir, &mut read, listed, differ).await;
    if differ {
        let compared = match &sides {
            Some((old, new)) => tables::compare(old, new),
            None => TablesCompared {
                changed: Vec::new(),
                may_differ: true,
            },
        };
        note_tables(&mut read.blackline, compared);
    }
    note_a_silent_difference(&mut read.blackline, differ);
    Ok(read)
}

/// Says in `blackline` what the comparison of the versions' table formatting found: the tables
/// that changed, with [`BlacklineNotice::CellFormatChanged`], and that it may differ where it
/// could not tell.
pub(crate) fn note_tables(blackline: &mut WordBlackline, compared: TablesCompared) {
    if !compared.changed.is_empty() {
        blackline.notices.push(BlacklineNotice::CellFormatChanged);
    }
    if compared.may_differ {
        blackline.notices.push(BlacklineNotice::MayDiffer);
    }
    blackline.changed_tables = compared.changed;
    order(&mut blackline.notices);
}

/// Adds [`BlacklineNotice::MayDiffer`] when the two files differ (`differ`) but the blackline
/// shows no change (in its body or a part) and nothing names what changed or says part of it
/// may be missing: stetdoc 0.9.3's compare marks no change for some differences, and "no
/// changes" must never stand for two different files.
pub(crate) fn note_a_silent_difference(blackline: &mut WordBlackline, differ: bool) {
    let shown = !blackline.parts.is_empty()
        || blackline
            .paragraphs
            .iter()
            .any(|paragraph| paragraph.change != ParagraphChange::Same);
    // Only a notice that already says something may be missing, or names what changed, makes
    // it needless; one about something else (hidden text, numbers) does not say the versions
    // differ.
    let said = !blackline.changed_links.is_empty()
        || !blackline.changed_tables.is_empty()
        || blackline.notices.iter().any(|notice| {
            matches!(
                notice,
                BlacklineNotice::MayDiffer
                    | BlacklineNotice::PartNotShown
                    | BlacklineNotice::ChangesOutsideText
                    | BlacklineNotice::HeaderOrFooterInOneVersion
                    | BlacklineNotice::CommentChanged
                    | BlacklineNotice::TextBoxChanged
                    | BlacklineNotice::CellFormatChanged
            )
        });
    if differ && !shown && !said {
        blackline.notices.push(BlacklineNotice::MayDiffer);
    }
}

/// `--author=<name>` as one argument, so a name that starts with `-` is never read as a flag.
fn author_arg(author: &str) -> OsString {
    OsString::from(format!("--author={author}"))
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
    let failed = |why: String| Failed {
        step,
        why,
        code: ErrorCode::BlacklineFailed,
    };
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
        .map_err(|err| failed(format!("it could not be started: {}", launch_why(&err))))?;
    let Launched {
        stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher
        .launch(&spec)
        .map_err(|err| failed(format!("it could not be started: {}", launch_why(&err))))?;
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
    let stopped = match ran {
        Some(Some((read, exit))) => Ok((read, exit)),
        None => Err(format!("it took longer than {} s", limit.as_secs())),
        Some(None) => Err("its stdout ran past 8 MiB; it was killed".to_owned()),
    };
    let (read, exit) = match stopped {
        Ok(ran) => ran,
        Err(why) => {
            // Past the deadline or the cap stetdoc is still running: stop its tree and wait for
            // it to exit, so the folder's guard never removes files it holds open (Windows
            // refuses that).
            let _outcome = handle.shutdown(timer, None).await;
            return Err(failed(why));
        }
    };
    drop(handle);
    read.map_err(|err| failed(format!("its stdout failed: {:?}", err.kind())))?;
    if exit.code() != Some(0) {
        let (word, message) = error_of(&out);
        return Err(Failed {
            code: refusal_code(&word, &message),
            ..failed(format!(
                "it exited with {:?} (error code {word})",
                exit.code()
            ))
        });
    }
    String::from_utf8(out).map_err(|_bad| failed("its stdout is not UTF-8".to_owned()))
}

/// Why a launch failed, for the log (ADR 0047): the step and the OS's `io::ErrorKind`, never
/// the OS's text or a path.
fn launch_why(error: &clauseo_process::Error) -> String {
    match error {
        clauseo_process::Error::InvalidSpec(_) => "invalid launch spec".to_owned(),
        clauseo_process::Error::NoWorkingFolder => "no working folder".to_owned(),
        clauseo_process::Error::Spawn { source, .. } => format!("spawn: {:?}", source.kind()),
        clauseo_process::Error::Os { what, source } => format!("{what}: {:?}", source.kind()),
        clauseo_process::Error::Closing => "the app is exiting".to_owned(),
    }
}

/// The `error.code` of stetdoc's JSON error on `stdout`, such as `malformed_input`, or `none`:
/// a short word of stetdoc's, never the document's text; and its `error.message`, which may
/// quote the document, so it is only matched ([`refusal_code`]), never logged or passed on.
fn error_of(stdout: &[u8]) -> (String, String) {
    let json = serde_json::from_slice::<serde_json::Value>(stdout).ok();
    let field = |pointer: &str| {
        json.as_ref()
            .and_then(|json| json.pointer(pointer))
            .and_then(serde_json::Value::as_str)
    };
    let code = field("/error/code")
        .filter(|code| code.len() <= 64 && code.bytes().all(|b| b.is_ascii_graphic()))
        .unwrap_or("none")
        .to_owned();
    (code, field("/error/message").unwrap_or_default().to_owned())
}

/// The words of stetdoc 0.9.3's `validation_failed` when the redline it made has a cell with no
/// paragraph at its end: its compare fails so when an inner table that ends its cell is removed
/// (stetdoc's own check, `t1: a table cell does not end with a paragraph.`).
const CELL_WITHOUT_PARAGRAPH: &str = "a table cell does not end with a paragraph";

/// The code a failed stetdoc run gives the caller, from its error `code` and `message`:
/// - `validation_failed` over a cell ([`CELL_WITHOUT_PARAGRAPH`]): `blackline.nested_table`;
/// - any other `validation_failed`, and `unsupported`: `blackline.cannot_compare`;
/// - `malformed_input`, `corrupt_complement`, `unsupported_version`: `blackline.not_readable`;
/// - `limit_exceeded`: `blackline.too_large`;
/// - anything else (`io_error`, `internal`, no JSON): `blackline.failed`, which may pass.
pub(crate) fn refusal_code(code: &str, message: &str) -> ErrorCode {
    match code {
        "validation_failed" if message.contains(CELL_WITHOUT_PARAGRAPH) => {
            ErrorCode::BlacklineNestedTable
        }
        "validation_failed" | "unsupported" => ErrorCode::BlacklineCannotCompare,
        "malformed_input" | "corrupt_complement" | "unsupported_version" => {
            ErrorCode::BlacklineNotReadable
        }
        "limit_exceeded" => ErrorCode::BlacklineTooLarge,
        _ => ErrorCode::BlacklineFailed,
    }
}

mod cross;
mod stories;

#[cfg(test)]
mod tests;
