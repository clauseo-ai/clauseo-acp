//! A Word file of an earlier version as text: copied on the matter's thread (capped at the
//! matter's size cap) into a private folder, `<data>/tmp/versions-<id>/version.docx` ([`copy_out`]), then shown by
//! `stetdoc view --all` off the matter's thread ([`view`]), within stetdoc's read limits (10 s,
//! 16 MiB of output), through a passthrough launcher with a clean environment, as tracked
//! changes run it. The folder goes when its [`Copied`] drops, after the run. The text of the last
//! Word file read is kept by content ([`Kept`]), so paging it runs stetdoc once.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_matters::Matter;
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_profile::skills::stetdoc::{RunFailure, RunOptions, run_for};
use clauseo_protocol::{CheckpointId, ContentId, MatterPath};

use super::lock;
use super::texts::Refusal;
use crate::clock::{Clock, ProcessTimer};
use crate::matter_worker::Capped;
use crate::revisions::stetdoc::{READ_LIMIT, READ_STDOUT_MAX, env};

/// The prefix of each copy's folder in `<data>/tmp/`; the blackline's start-up sweep removes
/// one a crash left.
pub(crate) const DIR_PREFIX: &str = "versions-";

/// The copy's name in its folder.
const COPY: &str = "version.docx";

/// The longest text [`Kept`] holds; a longer one is shown again for each page.
const KEPT_MAX: usize = 4 * 1024 * 1024;

/// The text of the last Word file read, by its content.
#[derive(Debug, Default)]
pub(super) struct Kept {
    last: Mutex<Option<(ContentId, Arc<str>)>>,
}

impl Kept {
    /// The kept text of `content`, if it is the one kept.
    pub(super) fn get(&self, content: &ContentId) -> Option<Arc<str>> {
        lock(&self.last)
            .as_ref()
            .filter(|(kept, _)| kept == content)
            .map(|(_, text)| Arc::clone(text))
    }

    /// Keeps `text` as `content`'s, when it is not too long.
    pub(super) fn put(&self, content: ContentId, text: &Arc<str>) {
        if text.len() <= KEPT_MAX {
            *lock(&self.last) = Some((content, Arc::clone(text)));
        }
    }
}

/// A Word file's private copy: its folder is removed when this drops.
#[derive(Debug)]
pub(super) struct Copied {
    dir: PathBuf,
}

impl Copied {
    fn file(&self) -> PathBuf {
        self.dir.join(COPY)
    }
}

impl Drop for Copied {
    fn drop(&mut self) {
        if let Err(error) = crate::write::remove_tree(&self.dir) {
            tracing::warn!(
                kind = ?error.kind(),
                "a version's Word copy could not be removed"
            );
        }
    }
}

/// A fresh copy folder's path, not made yet: `<data>/tmp/versions-<id>`, the id a v7 uuid of
/// the host clock's time, so the start-up sweep tells this run's from an earlier one's.
pub(super) fn copy_dir(data_dir: &Path, clock: &dyn Clock) -> PathBuf {
    let millis = clock.now().wall.as_millis();
    let secs = u64::try_from(millis.div_euclid(1000)).unwrap_or_default();
    let nanos = u32::try_from(millis.rem_euclid(1000) * 1_000_000).unwrap_or_default();
    let id = uuid::Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, secs, nanos));
    data_dir
        .join("tmp")
        .join(format!("{DIR_PREFIX}{}", id.simple()))
}

/// On the matter's thread: `path` as version `version` holds it, streamed into the new private
/// folder `dir` through [`Capped`] at `cap` (the matter's size cap, as the blackline's copies
/// take theirs): past it the copy is refused as [`Refusal::WordFailed`]. On any failure the
/// folder is gone before this returns.
pub(super) fn copy_out(
    matter: &Matter,
    version: CheckpointId,
    path: &MatterPath,
    dir: PathBuf,
    cap: u64,
) -> Result<Copied, Refusal> {
    let failed = |what: &str, error: &std::io::Error| {
        tracing::warn!(what, kind = ?error.kind(), "a version's Word copy could not be written");
        Refusal::WordFailed
    };
    if let Some(parent) = dir.parent() {
        crate::write::create_dir_all(parent).map_err(|error| failed("its folder", &error))?;
    }
    crate::write::create_private_dir(&dir).map_err(|error| failed("its folder", &error))?;
    let copied = Copied { dir };
    let file = crate::write::create_new_file(&copied.file())
        .map_err(|error| failed("the copy", &error))?;
    let mut out = Capped::new(file, cap);
    match matter.read_file(version, path, &mut out) {
        Ok(_) => Ok(copied),
        Err(_over) if out.over() => {
            tracing::warn!("a version's Word file is past the size cap");
            Err(Refusal::WordFailed)
        }
        Err(error) => {
            tracing::warn!(error = %crate::log_file::Cause(&error), "a version's Word file did not read");
            Err(Refusal::NotRead)
        }
    }
}

/// Off the matter's thread: `stetdoc view --all` of the copy, its stdout as the text. A run
/// that does not start, ends with another code than 0, runs past its limits or prints what is
/// not UTF-8 is [`Refusal::WordFailed`]; the log keeps the kind and the code only.
pub(super) fn view(
    program: &Path,
    clock: &Arc<dyn Clock>,
    copied: &Copied,
) -> Result<String, Refusal> {
    let launcher = PassthroughLauncher::new(SpawnOptions::new().clear_env().end_with_parent());
    let timer = ProcessTimer(Arc::clone(clock));
    let options = RunOptions {
        limit: READ_LIMIT,
        stdout_max: READ_STDOUT_MAX,
        env: env(
            cfg!(windows),
            |name| std::env::var_os(name),
            crate::agent::loads::case_law::windows_directory,
            &copied.dir,
        ),
    };
    let args: [OsString; 3] = [
        "view".into(),
        "--all".into(),
        copied.file().into_os_string(),
    ];
    let ran = futures::executor::block_on(run_for(&launcher, &timer, program, &args, &options));
    match ran {
        Ok(ran) if ran.code == Some(0) => String::from_utf8(ran.stdout).map_err(|_not_utf8| {
            tracing::warn!("stetdoc's view of a version was not UTF-8");
            Refusal::WordFailed
        }),
        Ok(ran) => {
            tracing::warn!(exit = ?ran.code, "stetdoc's view of a version failed");
            Err(Refusal::WordFailed)
        }
        Err(failure) => {
            let kind = match failure {
                RunFailure::Launch(_) => "launch",
                RunFailure::TimedOut => "timed_out",
                RunFailure::TooMuchOutput { .. } => "too_much_output",
                RunFailure::Read(_) => "read",
            };
            tracing::warn!(kind, "stetdoc's view of a version did not finish");
            Err(Refusal::WordFailed)
        }
    }
}
