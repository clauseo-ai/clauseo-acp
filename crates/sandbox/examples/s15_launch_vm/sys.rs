//! The harness's file writes, process starts and clock: every call `clippy.toml` keeps out of
//! the crate, each in one small function here, so the allowlist names each once. This is an
//! example run by hand on a test VM and in CI, not the product: it writes its own folders and
//! starts the runner itself.

use std::io::{self, Write as _};
use std::path::Path;
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

/// One line on stdout, flushed: the step's report.
pub(crate) fn say(line: &str) {
    let mut out = io::stdout().lock();
    drop(writeln!(out, "{line}").and_then(|()| out.flush()));
}

/// Makes `path` and its missing parents.
#[expect(
    clippy::disallowed_methods,
    reason = "the harness makes its own test folders"
)]
pub(crate) fn make_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Writes `bytes` to `path`, replacing it.
#[expect(
    clippy::disallowed_methods,
    reason = "the harness writes its manifest, the mock and codex's config"
)]
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)
}

/// Copies the file `from` to `to`.
#[expect(
    clippy::disallowed_methods,
    reason = "the harness copies the runner, itself, node and codex into its bin folder"
)]
pub(crate) fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::copy(from, to).map(drop)
}

/// Makes `link` a hard link to the file `target`.
#[expect(
    clippy::disallowed_methods,
    reason = "the m3-hardlink row links its own fixture into A's state folder"
)]
pub(crate) fn hard_link(target: &Path, link: &Path) -> io::Result<()> {
    std::fs::hard_link(target, link)
}

/// Makes `link` a symbolic link to the file `target` (unprivileged: developer mode).
#[expect(
    clippy::disallowed_methods,
    reason = "the m3-hardlink row's symbolic credential link, to its own fixture"
)]
pub(crate) fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

/// Removes the file `path`.
#[expect(
    clippy::disallowed_methods,
    reason = "cleanup removes the files the manifest lists"
)]
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Removes the folder `path` and everything in it.
#[expect(
    clippy::disallowed_methods,
    reason = "cleanup removes the folders the manifest lists, each by its full path"
)]
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Starts `command`.
#[expect(
    clippy::disallowed_methods,
    reason = "the harness starts the runner, the mock and its probes' children itself"
)]
pub(crate) fn start(command: &mut Command) -> io::Result<Child> {
    command.spawn()
}

/// Runs `command` to its end and gives its output (`icacls`, which ends at once).
#[expect(
    clippy::disallowed_methods,
    reason = "icacls runs to its end in a second"
)]
pub(crate) fn output(command: &mut Command) -> io::Result<Output> {
    command.output()
}

/// The time now, for timeouts and wall times.
#[expect(
    clippy::disallowed_methods,
    reason = "the harness times its rows and the logons"
)]
pub(crate) fn now() -> Instant {
    Instant::now()
}

/// Sleeps for `duration`: a probe's wait, and the polls of a child's exit.
#[expect(
    clippy::disallowed_methods,
    reason = "the probe waits, and the harness polls a child's exit"
)]
pub(crate) fn sleep(duration: Duration) {
    std::thread::sleep(duration);
}
