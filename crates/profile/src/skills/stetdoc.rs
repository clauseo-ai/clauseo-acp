//! stetdoc's skill: the text the bundled `stetdoc skills get core` prints, rendered as
//! `stetdoc/SKILL.md` with `stetdoc --version`'s first line as its version.
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
use futures::future::{Either, join3, select};

use super::{SkillFile, SkillSource, is_current};
use crate::error::ProfileError;

/// The skill's name.
pub const STETDOC: &str = "stetdoc";

/// The one file of stetdoc's skill.
const SKILL_FILE: &str = "SKILL.md";

/// How long one stetdoc run may take (it starts in about 5 ms).
pub const RUN_LIMIT: Duration = Duration::from_secs(10);

/// The most bytes of stdout one run keeps.
const STDOUT_MAX: u64 = 1024 * 1024;

/// stetdoc's skill for the bundled binary: runs `stetdoc --version`; when `cached` (a skills
/// folder a previous run wrote) already holds that version's skill, unchanged, takes its text
/// from there, else runs `stetdoc skills get core`. So `skills get core` runs again only when
/// the version changes or the file drifted. Starts stetdoc and reads files: off the host thread.
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
    Ok(SkillSource::verbatim(
        STETDOC,
        &version,
        vec![SkillFile {
            path: SKILL_FILE.to_owned(),
            bytes: text,
        }],
    ))
}

/// stetdoc's stdout for `args`, run in stetdoc's own folder with nothing added to the
/// environment, within [`RUN_LIMIT`]. An exit other than 0, stdout past 1 MiB or not UTF-8, a
/// failed launch or the deadline is an error.
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
            let (read, _drained, exit) = join3(
                limited.read_to_end(&mut out),
                stderr.drain(),
                handle.exited(),
            )
            .await;
            (read, exit)
        };
        match select(pin!(work), pin!(timer.after(RUN_LIMIT))).await {
            Either::Left((ran, _)) => Some(ran),
            Either::Right(((), _)) => None,
        }
    };
    // Dropping the handle kills the tree, if anything of it is left.
    drop(handle);
    let (read, exit) = ran.ok_or_else(|| stetdoc_error(args, "it timed out"))?;
    read.map_err(|err| stetdoc_error(args, format!("its stdout failed: {err}")))?;
    if exit.code() != Some(0) {
        return Err(stetdoc_error(
            args,
            format!("it exited with {:?}: {}", exit.code(), exit.stderr_tail()),
        ));
    }
    if u64::try_from(out.len()).unwrap_or(u64::MAX) > STDOUT_MAX {
        return Err(stetdoc_error(args, "its stdout ran past 1 MiB"));
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
