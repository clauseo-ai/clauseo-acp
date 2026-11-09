//! The outer runner's launch file: read once, through a handle closed before the read returns,
//! then removed, and the data folder it sits in. The launcher deletes the file once the outer
//! connects its denial pipe (`start.rs`), so nothing here may keep it open.

use std::path::{Path, PathBuf};

use super::super::{LAUNCH_CAP, LaunchMessage, RUN_DIR, RunExit, is_launch_name};
use super::{Failure, os};
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::runner::stderr::{self, Detail, RunnerKind};
use crate::windows::{private_dir, write};

/// Reads the launch file, if the desktop user owns it and only the desktop user, `SYSTEM` and
/// `Administrators` may open it, then removes it.
pub(super) fn read_launch(path: &Path) -> Result<LaunchMessage, Failure> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !is_launch_name(name) {
        return Err(Failure::new(
            RunExit::Usage,
            "the launch file's name is not a launch file's",
        ));
    }
    let bytes = private_dir::read_unchanged(path, LAUNCH_CAP)
        .map_err(|e| Failure::new(RunExit::BadLaunch, Detail::new(|| os("launch file", &e))))?
        .ok_or_else(|| {
            Failure::new(
                RunExit::BadLaunch,
                "the launch file is missing or not the desktop user's alone",
            )
        })?;
    if let Err(error) = write::remove_owned_file(path) {
        let detail = Detail::new(|| {
            format!(
                "clauseo-sandbox-run: the launch file stays: {}",
                error.kind()
            )
        });
        stderr::notice(RunnerKind::LaunchFileKept, detail);
    }
    LaunchMessage::parse(&bytes)
        .map_err(|e| Failure::new(RunExit::BadLaunch, Detail::new(|| e.to_string())))
}

/// `<data_dir>` from `<data_dir>\sandbox\run\<id>.json`.
pub(super) fn data_dir_of(launch_file: &Path) -> Result<PathBuf, Failure> {
    let run = launch_file.parent();
    let sandbox = run
        .filter(|run| run.file_name().is_some_and(|name| name == RUN_DIR))
        .and_then(Path::parent);
    sandbox
        .filter(|sandbox| sandbox.file_name().is_some_and(|name| name == SANDBOX_DIR))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            Failure::new(
                RunExit::Usage,
                "the launch file isn't in <data_dir>\\sandbox\\run",
            )
        })
}
