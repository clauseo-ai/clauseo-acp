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

/// The most bytes of stdout one run keeps.
const STDOUT_MAX: u64 = 1024 * 1024;

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
    let cwd = stetdoc.parent().unwrap_or(stetdoc).to_owned();
    let spec = LaunchSpec::new(
        stetdoc.to_owned(),
        args.iter().map(OsString::from).collect(),
        BTreeMap::new(),
        cwd,
    )
    .map_err(|err| stetdoc_error(args, err.to_string()))?;
    let Launched {
        stdin,
        stdout,
        stderr,
        mut handle,
        ..
    } = launcher
        .launch(&spec)
        .map_err(|err| stetdoc_error(args, err.to_string()))?;
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
        match select(pin!(work), pin!(timer.after(RUN_LIMIT))).await {
            Either::Left((ran, _)) => Some(ran),
            Either::Right(((), _)) => None,
        }
    };
    // Dropping the handle kills the tree, if anything of it is left: a run past the cap or the
    // deadline is still writing.
    drop(handle);
    let (read, exit) = ran
        .ok_or_else(|| stetdoc_error(args, "it timed out"))?
        .ok_or_else(|| stetdoc_error(args, "its stdout ran past 1 MiB; it was killed"))?;
    read.map_err(|err| stetdoc_error(args, format!("its stdout failed: {err}")))?;
    if exit.code() != Some(0) {
        return Err(stetdoc_error(
            args,
            format!("it exited with {:?}: {}", exit.code(), exit.stderr_tail()),
        ));
    }
    String::from_utf8(out)
        .map_err(|err| stetdoc_error(args, format!("its stdout is not UTF-8: {err}")))
}

fn stetdoc_error(args: &[&str], reason: impl Into<String>) -> ProfileError {
    ProfileError::Stetdoc {
        args: args.join(" "),
        reason: reason.into(),
    }
}
