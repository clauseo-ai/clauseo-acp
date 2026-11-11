//! stetdoc's skill: the text the bundled `stetdoc skills get core` prints, rendered as
//! `stetdoc/SKILL.md` with `stetdoc --version`'s first line as its version. Its frontmatter is
//! rewritten to `name: stetdoc` and stetdoc's `description` alone ([`skill_source`]), so no
//! `allowed-tools`, `hooks` or other key of stetdoc's reaches an agent. stetdoc 0.9.3 prints the
//! skill's body alone, with no frontmatter: Clauseo then writes the frontmatter itself, its
//! description stetdoc's first `# ` heading (or [`FALLBACK_DESCRIPTION`]), the body unchanged.
//! [`version`] and [`load_skill_at`] are the two halves of [`load_skill`], so a caller can tell
//! a stetdoc that does not run from a skill text that is refused.
//!
//! [`load_skill`] runs when the app builds the profile, off the host thread ([`start`] runs
//! it once more in the background after a slow first run); the actor-side calls only hand out
//! what it loaded. stetdoc starts through `clauseo-process`, like
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

pub mod start;

/// The skill's name.
pub const STETDOC: &str = "stetdoc";

/// The one file of stetdoc's skill.
const SKILL_FILE: &str = SKILL_MD;

/// The description stetdoc's skill gets when its text has no frontmatter and no `# ` heading
/// near its top ([`skill_source`]).
pub const FALLBACK_DESCRIPTION: &str =
    "Edit Word documents with real tracked changes using the stetdoc command.";

/// How many non-empty lines from the top of a text with no frontmatter are searched for the
/// `# ` heading that becomes the description.
const HEADING_WITHIN: usize = 5;

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
    let version = version(launcher, timer, stetdoc).await?;
    load_skill_at(launcher, timer, stetdoc, &version, cached).await
}

/// The first line `stetdoc --version` prints, trimmed. A run that fails, or prints no version,
/// is an error: this stetdoc does not work.
pub async fn version<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Path,
) -> Result<String, ProfileError> {
    version_within(launcher, timer, stetdoc, RUN_LIMIT)
        .await
        .map_err(RunError::into_error)
}

/// [`version`] within `limit`, telling a run that only passed its limit from one that failed.
pub async fn version_within<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Path,
    limit: Duration,
) -> Result<String, RunError> {
    let printed = run_within(launcher, timer, stetdoc, &["--version"], limit).await?;
    let version = printed.lines().next().unwrap_or_default().trim().to_owned();
    if version.is_empty() {
        return Err(RunError::Failed(stetdoc_error(
            &["--version"],
            "it printed no version",
        )));
    }
    Ok(version)
}

/// [`load_skill`] once [`version`] gave `version`: the skill from `cached` when it holds this
/// version's, unchanged, else from `stetdoc skills get core`.
pub async fn load_skill_at<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Path,
    version: &str,
    cached: &Path,
) -> Result<SkillSource, ProfileError> {
    let held = cached.join(STETDOC).join(SKILL_FILE);
    let text = if is_current(cached, STETDOC, version)? {
        std::fs::read(&held).map_err(|err| ProfileError::io("read", &held, err))?
    } else {
        run(launcher, timer, stetdoc, &["skills", "get", "core"])
            .await?
            .into_bytes()
    };
    skill_source(version, &text)
}

/// stetdoc's skill from the text `skills get core` printed (or a cached copy, already in this
/// form): `name: stetdoc`, a `description`, then the body. A leading UTF-8 BOM is dropped.
///
/// - A text with a frontmatter (its first line `---`, ending in LF or CRLF) keeps stetdoc's
///   `description`; every other key is dropped. A frontmatter with no `description` is an
///   error for this skill alone. A CRLF frontmatter's text is read with LF line ends.
/// - A text with none (stetdoc 0.9.3 prints the body alone) gets one from Clauseo: the
///   description is the first `# ` heading in its first [`HEADING_WITHIN`] non-empty lines,
///   trimmed, or [`FALLBACK_DESCRIPTION`]; the body is the text, unchanged.
///
/// Text that is not UTF-8 is an error for this skill alone.
pub fn skill_source(version: &str, text: &[u8]) -> Result<SkillSource, ProfileError> {
    let text = std::str::from_utf8(text)
        .map_err(|err| ProfileError::skill(STETDOC, format!("its text is not UTF-8: {err}")))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rendered = if text.starts_with("---\r\n") {
        with_own_frontmatter(&text.replace("\r\n", "\n"))?
    } else if text.starts_with("---\n") {
        with_own_frontmatter(text)?
    } else {
        let description = first_heading(text).unwrap_or(FALLBACK_DESCRIPTION);
        format!("{}{text}", frontmatter::render(STETDOC, description))
    };
    Ok(SkillSource::verbatim(
        STETDOC,
        version,
        vec![SkillFile {
            path: SKILL_FILE.to_owned(),
            bytes: rendered.into_bytes(),
        }],
    ))
}

/// `text`, which starts with a `---\n` line, as `SKILL.md`: `name: stetdoc` and its own
/// `description` alone, then its body.
fn with_own_frontmatter(text: &str) -> Result<String, ProfileError> {
    let (description, body) = frontmatter::lenient_description(text)
        .map_err(|reason| ProfileError::skill(STETDOC, reason))?;
    // A literal (`|`) description holds newlines; the rendered frontmatter is one line.
    let description = description
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!(
        "{}{body}",
        frontmatter::render(STETDOC, &description)
    ))
}

/// The first `# ` heading's text, trimmed, in the first [`HEADING_WITHIN`] non-empty lines of
/// `text` (LF or CRLF line ends); none when there is none or it is empty.
fn first_heading(text: &str) -> Option<&str> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .take(HEADING_WITHIN)
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|heading| !heading.is_empty())
}

/// Why a [`run_within`] gave no text to use: the run only passed its limit (a stetdoc that may
/// work but is slow, such as the first run of a new program that the OS scans), or it failed.
#[derive(Debug)]
pub enum RunError {
    /// It ran past its limit and was killed with its tree.
    TimedOut(ProfileError),
    /// It did not start, exited with an error, or printed what cannot be used.
    Failed(ProfileError),
}

impl RunError {
    /// The error, whichever way the run went.
    #[must_use]
    pub fn into_error(self) -> ProfileError {
        match self {
            Self::TimedOut(error) | Self::Failed(error) => error,
        }
    }
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
    run_within(launcher, timer, stetdoc, args, RUN_LIMIT)
        .await
        .map_err(RunError::into_error)
}

/// [`run`] within `limit`: the deadline is [`RunError::TimedOut`], any other failure
/// [`RunError::Failed`].
pub async fn run_within<T: Timer>(
    launcher: &dyn Launcher,
    timer: &T,
    stetdoc: &Path,
    args: &[&str],
    limit: Duration,
) -> Result<String, RunError> {
    let os_args: Vec<OsString> = args.iter().map(OsString::from).collect();
    let ran = run_for(launcher, timer, stetdoc, &os_args, &RunOptions::new(limit))
        .await
        .map_err(|failure| {
            let error = stetdoc_error(args, failure.to_string());
            if failure == RunFailure::TimedOut {
                RunError::TimedOut(error)
            } else {
                RunError::Failed(error)
            }
        })?;
    if ran.code != Some(0) {
        return Err(RunError::Failed(stetdoc_error(
            args,
            format!("it exited with {:?}: {}", ran.code, ran.stderr_tail),
        )));
    }
    String::from_utf8(ran.stdout).map_err(|err| {
        RunError::Failed(stetdoc_error(
            args,
            format!("its stdout is not UTF-8: {err}"),
        ))
    })
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
