//! The check that a codex-acp version still takes the network patch: [`check_codex_patch`], run
//! by the engine watch on an `npm ci --ignore-scripts` of a pair before the version is accepted.
//!
//! It reads `dist/index.js` from the install, applies [`patch`] to the bytes in memory (the same
//! pattern the install uses), writes the patched bytes to a temp file beside the real one and runs
//! `node --check` on it. The install itself is never changed.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::time::Duration;

use clauseo_process::{
    AsyncIoTimer, LaunchSpec, Launched, Launcher as _, PassthroughLauncher, SpawnOptions,
    Timer as _,
};
use futures::future::{Either, join3, select};

use super::{PatchError, patch, rel};
use crate::config::NodeSearch;

#[cfg(test)]
mod tests;

/// The file checked, `/`-separated below the `node_modules` folder given.
const IN_MODULES: &str = "@agentclientprotocol/codex-acp/dist/index.js";
/// How long `node --check` may run before it is ended.
const CHECK_LIMIT: Duration = Duration::from_secs(60);

/// Why [`check_codex_patch`] refused an install's codex-acp.
#[derive(Debug)]
pub struct CheckError {
    file: PathBuf,
    step: Step,
}

/// The step that failed.
#[derive(Debug)]
enum Step {
    /// The file could not be read.
    Read(io::Error),
    /// A mode's pattern is not there exactly once, or its policy or network line is not.
    Pattern(PatchError),
    /// The patched copy could not be written beside the file.
    Temp(io::Error),
    /// No `node` on `PATH`.
    NodeMissing,
    /// `node` could not be run to its end.
    NodeRun(String),
    /// `node --check` refused the patched file, with the line of its stderr that names the error.
    Syntax(String),
}

impl CheckError {
    fn new(file: &Path, step: Step) -> CheckError {
        CheckError {
            file: file.to_path_buf(),
            step,
        }
    }

    /// The step that failed: a mode's name (`workspace-write` or `agent`), or `read`,
    /// `temp file` or `node --check`.
    pub fn step(&self) -> &str {
        match &self.step {
            Step::Read(_) => "read",
            Step::Pattern(PatchError::Anchor(mode)) => mode,
            Step::Temp(_) => "temp file",
            Step::NodeMissing | Step::NodeRun(_) | Step::Syntax(_) => "node --check",
        }
    }

    /// The file that was checked.
    pub fn file(&self) -> &Path {
        &self.file
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.step {
            Step::Read(err) => write!(f, "cannot read it: {err}"),
            Step::Pattern(err) => write!(f, "{err}"),
            Step::Temp(err) => write!(f, "cannot write the patched copy beside it: {err}"),
            Step::NodeMissing => write!(f, "no node on PATH to run node --check with"),
            Step::NodeRun(reason) => write!(f, "cannot run node --check: {reason}"),
            Step::Syntax(line) => write!(f, "the patched file does not parse: {line}"),
        }
    }
}

impl std::error::Error for CheckError {}

/// Checks codex-acp's `dist/index.js` in `node_modules` (an `npm ci --ignore-scripts` of a pair
/// that names codex-acp): each mode's pattern must match exactly once, and the file with the patch
/// applied must pass `node --check`. Returns the file checked. Nothing in the install changes.
pub fn check_codex_patch(node_modules: &Path) -> Result<PathBuf, CheckError> {
    let file = std::path::absolute(node_modules)
        .map(|dir| rel(&dir, IN_MODULES))
        .map_err(|err| CheckError::new(node_modules, Step::Read(err)))?;
    let bytes = std::fs::read(&file).map_err(|err| CheckError::new(&file, Step::Read(err)))?;
    let patched = pattern_step(&bytes).map_err(|step| CheckError::new(&file, step))?;
    syntax_step(&file, &patched).map_err(|step| CheckError::new(&file, step))?;
    Ok(file)
}

/// The pattern step: `bytes` with both modes patched, or as they are when both already are.
fn pattern_step(bytes: &[u8]) -> Result<Vec<u8>, Step> {
    match patch(bytes) {
        Ok(Some(patched)) => Ok(patched),
        Ok(None) => Ok(bytes.to_vec()),
        Err(err) => Err(Step::Pattern(err)),
    }
}

/// The syntax step: `bytes` in a temp file beside `file`, which `node --check` reads. The temp
/// file is beside it so node treats it as the file would be treated (its package's module type).
fn syntax_step(file: &Path, bytes: &[u8]) -> Result<(), Step> {
    let node = node_on_path().ok_or(Step::NodeMissing)?;
    let temp = Temp::beside(file, bytes).map_err(Step::Temp)?;
    let spec = LaunchSpec::new(
        node,
        vec!["--check".into(), temp.0.clone().into()],
        BTreeMap::new(),
        std::env::temp_dir(),
    )
    .map_err(|err| Step::NodeRun(err.to_string()))?;
    let launched = PassthroughLauncher::new(SpawnOptions::new())
        .launch(&spec)
        .map_err(|err| Step::NodeRun(err.to_string()))?;
    match futures::executor::block_on(finish(launched)) {
        None => Err(Step::NodeRun(format!(
            "it did not end within {} s",
            CHECK_LIMIT.as_secs()
        ))),
        Some((Some(0), _)) => Ok(()),
        Some((Some(_), stderr)) => Err(Step::Syntax(error_line(&stderr))),
        Some((None, _)) => Err(Step::NodeRun("a signal ended it".to_owned())),
    }
}

/// `node --check`, run to its exit or to [`CHECK_LIMIT`]: its stdout is drained, its stderr
/// driven, and its exit code and stderr tail returned (`None` on the limit).
async fn finish(launched: Launched) -> Option<(Option<i32>, String)> {
    let Launched {
        stdout,
        stderr,
        mut handle,
        ..
    } = launched;
    let done = {
        let work = pin!(async {
            let mut sink = futures::io::sink();
            let (_, _, report) = join3(
                futures::io::copy(stdout, &mut sink),
                stderr.drain(),
                handle.exited(),
            )
            .await;
            (report.code(), report.stderr_tail())
        });
        match select(work, pin!(AsyncIoTimer.after(CHECK_LIMIT))).await {
            Either::Left((done, _)) => Some(done),
            Either::Right(((), _)) => None,
        }
    };
    // Ended on the limit, the handle kills what is left of the tree here.
    drop(handle);
    done
}

/// The line of node's stderr that names the error, one line, or a fallback.
fn error_line(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .find(|line| line.contains("Error:") || line.contains("Error ["))
        .unwrap_or("node exited with an error")
        .to_owned()
}

/// Node as the first absolute `node` on `PATH`: the same candidates the user's Node is found by.
fn node_on_path() -> Option<PathBuf> {
    crate::node::candidates(&NodeSearch::from_env())
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// The patched copy beside the real file, removed when it is dropped.
struct Temp(PathBuf);

impl Temp {
    /// Writes `bytes` to `patchcheck-<pid>.js` beside `file`, so the name ends in `.js`.
    fn beside(file: &Path, bytes: &[u8]) -> io::Result<Temp> {
        let path = file.with_file_name(format!("patchcheck-{}.js", std::process::id()));
        crate::write::write_atomic(&path, bytes)?;
        Ok(Temp(path))
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        drop(crate::write::remove_file(&self.0));
    }
}
