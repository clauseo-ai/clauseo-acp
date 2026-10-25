//! The checks of the checkout's `target/` before the warm-up: cargo builds into `<root>/target`,
//! `target/debug` is a real folder when it is there, and `target/xtask-run` is there. With another
//! target or build dir (`CARGO_TARGET_DIR`, `build.target-dir`, `build.build-dir`), the warm-up
//! and the worktree's first build share one dir, and the first build reuses its output; with a
//! `target/debug` that is a symlink, `write::copy_into` copies the link and the worktree builds
//! into the checkout's output. Either way the fresh check passes and the number looks valid.

use super::super::verdict::Failure;
use super::steps::same_path;
use super::{APP, Worktree};
use crate::process::QuietRunner;
use crate::workspace;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// The fields of `cargo metadata` that name where cargo builds: final output, then the rest.
const DIRS: [&str; 2] = ["target_directory", "build_directory"];

/// The fix when cargo builds outside the checkout's own `target/`.
pub(crate) const SHARED_TARGET_FIX: &str = "build into the checkout's own target/ for this run: \
    unset `CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` and `CARGO_BUILD_BUILD_DIR`, and take \
    `build.target-dir` and `build.build-dir` out of the cargo config files read here \
    (`.cargo/config.toml` in the checkout or a folder above it, and `config.toml` in \
    `CARGO_HOME`); then run again";

/// The fix when xtask's build output is not in the checkout.
pub(crate) const XTASK_RUN_FIX: &str = "run `cargo xtask --help` from the repo root (the alias \
    builds xtask into target/xtask-run), then run again";

/// Fails unless both dirs `cargo metadata` names are `<root>/target`, `target/debug` is a
/// folder and no symlink (or not there yet: the warm-up then makes it), and `target/xtask-run`
/// is there.
pub(super) fn refuse_shared(
    worktree: &Worktree<'_>,
    runner: &mut QuietRunner<'_>,
) -> Result<(), Failure> {
    let cmd = workspace::metadata_cmd();
    let printed = workspace::run_metadata(worktree.root, runner).map_err(|error| {
        Failure::new(
            error,
            format!("run `{cmd}` from the repo root to see the error"),
        )
    })?;
    let metadata = serde_json::from_str::<serde_json::Value>(&printed).ok();
    let own = worktree.root.join("target");
    for field in DIRS {
        let named = metadata
            .as_ref()
            .and_then(|value| value.get(field)?.as_str());
        let Some(dir) = named else {
            return Err(Failure::new(
                format!("`{cmd}` printed no `{field}`"),
                format!("run `{cmd}` from the repo root to see what it prints"),
            ));
        };
        if !own_target(Path::new(dir), worktree.root) {
            return Err(Failure::new(
                format!(
                    "refusing to measure: cargo builds into {dir}, not {} (`{field}` of \
                     `{cmd}`), so the warm-up and the worktree's first build would share it \
                     and the first build would reuse its output",
                    own.display()
                ),
                SHARED_TARGET_FIX,
            ));
        }
    }
    let debug = own.join("debug");
    let shown = debug.display();
    match fs::symlink_metadata(&debug) {
        Ok(meta) if meta.is_dir() => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Ok(meta) => {
            let what = if meta.is_symlink() {
                "a symlink, not a folder"
            } else {
                "not a folder"
            };
            return Err(Failure::new(
                format!(
                    "refusing to measure: {shown} is {what}, so the worktree would not get \
                     its own copy of the build output"
                ),
                format!(
                    "move {shown} away (the warm-up, `cargo build -p {APP}`, then builds a \
                     real folder there), then run again"
                ),
            ));
        }
        Err(error) => {
            return Err(Failure::new(
                format!("could not read {shown}: {error}"),
                "fix what the error names, then run again",
            ));
        }
    }
    if fs::symlink_metadata(own.join("xtask-run")).is_err() {
        return Err(Failure::new(
            "target/xtask-run is not in this checkout",
            XTASK_RUN_FIX,
        ));
    }
    Ok(())
}

/// Whether `dir` is `<root>/target`: named `target`, in a folder that is `root` (`same_path`,
/// so as Windows spells it either way, and with `dir` not made yet).
pub(crate) fn own_target(dir: &Path, root: &Path) -> bool {
    dir.file_name().is_some_and(|name| name == "target")
        && dir.parent().is_some_and(|parent| same_path(parent, root))
}
