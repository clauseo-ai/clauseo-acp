//! stetdoc's skill: the text the bundled `stetdoc skills get core` prints, rendered as
//! `stetdoc/SKILL.md` with `stetdoc --version`'s first line as its version. Its frontmatter is
//! rewritten to `name: stetdoc` and stetdoc's `description` alone ([`skill_source`]), so no
//! `allowed-tools`, `hooks` or other key of stetdoc's reaches an agent.
//!
//! [`load_skill`] runs once, when the app builds the profile, off the host thread; the
//! actor-side calls only hand out what it loaded. stetdoc starts through `clauseo-process`, like
//! every child: an absolute program, no shell, a tree of its own killed on a timeout.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{LaunchSpec, Launched, Launcher, Timer};
use futures::AsyncReadExt as _;
use futures::future::{Either, join, select};

use super::{SKILL_MD, SkillFile, SkillSource, frontmatter, is_current};
use crate::error::ProfileError;

/// The skill's name.
pub const STETDOC: &str = "stetdoc";

/// The one file of stetdoc's skill.
const SKILL_FILE: &str = SKILL_MD;

/// How long one stetdoc run may take (it starts in about 5 ms).
pub const RUN_LIMIT: Duration = Duration::from_secs(10);

/// The most bytes of stdout one skill-loading run keeps.
pub const STDOUT_MAX: u64 = 1024 * 1024;

/// What one [`run_for`] gets: its deadline, the most stdout it keeps, and the variables added
/// to the environment the launcher gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    /// Past it the run's tree is killed.
    pub limit: Duration,
    /// The most bytes of stdout kept; past it the run's tree is killed.
    pub stdout_max: u64,
    /// Added to the launcher's environment.
    pub env: BTreeMap<OsString, OsString>,
}

impl RunOptions {
    /// `limit`, [`STDOUT_MAX`] and nothing added to the environment.
    #[must_use]
    pub const fn new(limit: Duration) -> Self {
        Self {
            limit,
            stdout_max: STDOUT_MAX,
            env: BTreeMap::new(),
        }
    }
}

/// stetdoc's skill for the bundled binary: runs `stetdoc --version`; when `cached` (a skills
/// folder a previous run wrote) already holds that version's skill, unchanged, takes its text
/// from there, else runs `stetdoc skills get core`. So `skills get core` runs again only when
/// the version changes or the file drifted. Starts stetdoc and reads files: off the host thread.
/// An error here fails stetdoc's skill alone: the app logs it as a warning and builds the
/// profile with the other skills.
pub async fn load_skill<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Path,
    cached: &Path,
) -> Result<SkillSource, ProfileError> {
    let printed = run(launcher, timer, stetdoc, &["--version"]).await?;
    let version = printed.lines().next().unwrap_or_default().trim().to_owned();
    if version.is_empty() {
        return Err(stetdoc_error(&["--version"], "it printed no version"));
    }
    let held = cached.join(STETDOC).join(SKILL_FILE);
    let text = if is_current(cached, STETDOC, &version)? {
        std::fs::read(&held).map_err(|err| ProfileError::io("read", &held, err))?
    } else {
        run(launcher, timer, stetdoc, &["skills", "get", "core"])
            .await?
            .into_bytes()
    };
    skill_source(&version, &text)
}

/// stetdoc's skill from the text `skills get core` printed (or a cached copy, already in this
/// form): `name: stetdoc`, stetdoc's `description`, then its body. Every other frontmatter key
/// is dropped. Text that is not UTF-8 or has no `description` is an error for this skill alone.
pub fn skill_source(version: &str, text: &[u8]) -> Result<SkillSource, ProfileError> {
    let text = std::str::from_utf8(text)
        .map_err(|err| ProfileError::skill(STETDOC, format!("its text is not UTF-8: {err}")))?;
    let (description, body) = frontmatter::lenient_description(text)
        .map_err(|reason| ProfileError::skill(STETDOC, reason))?;
    // A literal (`|`) description holds newlines; the rendered frontmatter is one line.
    let description = description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let rendered = format!("{}{body}", frontmatter::render(STETDOC, &description));
    Ok(SkillSource::verbatim(
        STETDOC,
        version,
        vec![SkillFile {
            path: SKILL_FILE.to_owned(),
            bytes: rendered.into_bytes(),
        }],
    ))
}

/// stetdoc's stdout for `args`, run in stetdoc's own folder with nothing added to the
/// environment, within [`RUN_LIMIT`]. An exit other than 0, stdout past 1 MiB (stetdoc is
/// killed as soon as it passes) or not UTF-8, a failed launch or the deadline is an error.
pub async fn run<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Path,
    args: &[&str],
) -> Result<String, ProfileError> {
    let os_args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let ran = run_for(
        launcher,
        timer,
        stetdoc,
        &os_args,
        &RunOptions::new(RUN_LIMIT),
    )
    .await
    .map_err(|failure| stetdoc_error(args, failure.to_string()))?;
    if ran.code != Some(0) {
        return Err(stetdoc_error(
            args,
            format!("it exited with {:?}: {}", ran.code, ran.stderr_tail),
        ));
    }
    String::from_utf8(ran.stdout)
        .map_err(|err| stetdoc_error(args, format!("its stdout is not UTF-8: {err}")))
}

/// How one stetdoc run of [`run_for`] exited, whatever the code: stetdoc with `--json` prints
/// its refusals on stdout and exits with the refusal's code (`stetdoc capabilities`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The exit code; `None` when a signal ended it.
    pub code: Option<i32>,
    /// Everything it printed on stdout, at most [`RunOptions::stdout_max`].
    pub stdout: Vec<u8>,
    /// The end of its stderr, for the log.
    pub stderr_tail: String,
}

/// Why a [`run_for`] has no exit to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFailure {
    /// The program could not be started (missing, not a program, refused).
    Launch(String),
    /// It ran past its limit and was killed with its tree.
    TimedOut,
    /// Its stdout ran past `max` bytes and it was killed with its tree.
    TooMuchOutput {
        /// [`RunOptions::stdout_max`].
        max: u64,
    },
    /// Reading its stdout failed.
    Read(String),
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Launch(reason) => write!(f, "it could not start: {reason}"),
            Self::TimedOut => f.write_str("it timed out"),
            Self::TooMuchOutput { max } => {
                write!(
                    f,
                    "its stdout ran past {} MiB; it was killed",
                    max / (1024 * 1024)
                )
            }
            Self::Read(reason) => write!(f, "its stdout failed: {reason}"),
        }
    }
}

/// Runs stetdoc with `args` in its own folder, with `options.env` added to the environment the
/// launcher gives it, and returns how it exited, whatever the code, within `options.limit`.
/// Past the limit or `options.stdout_max` bytes of stdout its process tree is killed. Waits on
/// `timer` and stetdoc's pipes: run it off any thread that must stay responsive.
pub async fn run_for<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Path,
    args: &[OsString],
    options: &RunOptions,
) -> Result<Ran, RunFailure> {
    let (limit, max) = (options.limit, options.stdout_max);
    let cwd = stetdoc.parent().unwrap_or(stetdoc).to_owned();
    let spec = LaunchSpec::new(stetdoc.to_owned(), args.to_vec(), options.env.clone(), cwd)
        .map_err(|err| RunFailure::Launch(err.to_string()))?;
    let Launched {
        stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher
        .launch(&spec)
        .map_err(|err| RunFailure::Launch(err.to_string()))?;
    drop(stdin);
    let mut out = Vec::new();
    let ran = {
        let work = async {
            let mut limited = stdout.take(max.saturating_add(1));
            let mut drain = pin!(stderr.drain());
            let (read, drained) = {
                let reading = pin!(limited.read_to_end(&mut out));
                match select(reading, drain.as_mut()).await {
                    Either::Left((read, _)) => (read, false),
                    Either::Right((_, reading)) => (reading.await, true),
                }
            };
            if read.is_ok() && u64::try_from(out.len()).unwrap_or(u64::MAX) > max {
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
        .ok_or(RunFailure::TimedOut)?
        .ok_or(RunFailure::TooMuchOutput { max })?;
    read.map_err(|err| RunFailure::Read(err.to_string()))?;
    Ok(Ran {
        code: exit.code(),
        stdout: out,
        stderr_tail: exit.stderr_tail(),
    })
}

fn stetdoc_error(args: &[&str], reason: impl Into<String>) -> ProfileError {
    ProfileError::Stetdoc {
        args: args.join(" "),
        reason: reason.into(),
    }
}
