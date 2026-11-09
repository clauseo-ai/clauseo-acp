//! A tree that spares a browser (`LaunchSpec::spare_browsers`, Windows): the kill ends every
//! process of the job but one named like a browser, which keeps running after the handle drops
//! and the job's handle closes, as the lawyer's Edge must after a sign-in's Cancel or the quit.
//! The browser stand-in is a copy of `fake_agent` named `msedge.exe`, holding (`--hold`); its
//! sibling, `fake_agent` under its own name, must die. Waits poll with a bound, never sleep as
//! a sync.

#![cfg(windows)]
#![expect(
    clippy::disallowed_methods,
    reason = "the browser stand-in: a copy of fake_agent in the test's own temp folder"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_io::{Timer, block_on};
use clauseo_process::{LaunchSpec, SpawnOptions, image_and_parent, kill, running, spawn_in_tree};

/// How often the test looks again, and how many looks make its 10 s bound.
#[cfg(test)]
const POLL: Duration = Duration::from_millis(20);
#[cfg(test)]
const POLLS: u32 = 500;

/// Whether `check` holds within the bound.
#[cfg(test)]
fn within_bound(mut check: impl FnMut() -> bool) -> bool {
    for _ in 0..POLLS {
        if check() {
            return true;
        }
        block_on(Timer::after(POLL));
    }
    check()
}

/// The pid of the tree's process whose image file is named `name`, once it runs.
#[cfg(test)]
fn find(pids: &[u32], name: &str) -> Option<u32> {
    pids.iter().copied().find(|&pid| {
        image_and_parent(pid).is_some_and(|(exe, _)| {
            exe.file_name()
                .is_some_and(|file| file.eq_ignore_ascii_case(name))
        })
    })
}

/// A folder of the test's own under the system's temp folder, with no space in its path (the
/// command line below is not quoted).
#[cfg(test)]
fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("clauseo-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(!dir.to_string_lossy().contains(' '), "{}", dir.display());
    dir
}

#[test]
fn the_kill_spares_a_browser_and_ends_the_rest() {
    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    assert!(!bin.to_string_lossy().contains(' '), "{}", bin.display());
    let dir = folder("spare-browser");
    let edge = dir.join("msedge.exe");
    std::fs::copy(&bin, &edge).unwrap();
    let cmd: PathBuf = std::env::var_os("ComSpec").expect("ComSpec").into();
    let line = format!(
        "start /b {} --hold & {} --hold",
        edge.display(),
        bin.display()
    );
    let spec = LaunchSpec::new(
        cmd,
        vec!["/d".into(), "/c".into(), line.into()],
        BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap()
    .spare_browsers();
    let launched = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    let handle = launched.handle;
    let descendants = handle.descendants();
    let mut found = (None, None);
    let both = within_bound(|| {
        let pids = descendants.snapshot().pids().unwrap_or_default();
        found = (find(&pids, "msedge.exe"), find(&pids, file_name(&bin)));
        found.0.is_some() && found.1.is_some()
    });
    let (Some(browser), Some(other)) = found else {
        panic!("the tree never held both: {both}");
    };
    let root = handle.pid();
    // The drop kills the tree and closes the job's handle: both would end a browser in it.
    drop(handle);
    drop(launched.stdout);
    let rest_gone = within_bound(|| !running(other) && !running(root));
    let browser_runs = running(browser);
    kill(browser);
    within_bound(|| !running(browser));
    drop(std::fs::remove_dir_all(&dir));
    assert!(rest_gone, "the tree's other processes still run");
    assert!(browser_runs, "the browser was ended with the tree");
}

/// The file name of `path`, as a `&str`.
#[cfg(test)]
fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|name| name.to_str()).unwrap()
}
