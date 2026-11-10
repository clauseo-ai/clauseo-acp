//! A tree that spares a browser (`LaunchSpec::spare_browsers`, Windows): the kill ends every
//! process of the job but one named like a browser, which keeps running after the handle drops
//! and the job's handle closes, as the lawyer's Edge must after a sign-in's Cancel or the quit.
//! The browser stand-in is a copy of `fake_agent` named `msedge.exe`, holding (`--hold`); its
//! sibling, `fake_agent` under its own name, must die. `cmd` makes and removes the copy and
//! its folder, so no file call of the test's own changes a file. Waits poll with a bound, never
//! sleep as a sync. The cases of a process that cannot be ended, a run named like a browser,
//! and a process joining after the last listing are `os/windows/spare/tests.rs`.

#![cfg(windows)]

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

/// A folder name of the test's own under the system's temp folder, with no space in its path
/// (the command lines below are not quoted). `cmd` makes it.
#[cfg(test)]
fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("clauseo-{tag}-{}", std::process::id()));
    assert!(!dir.to_string_lossy().contains(' '), "{}", dir.display());
    dir
}

/// Runs `cmd /d /c line` and waits for it.
#[cfg(test)]
fn cmd(line: &str) {
    let shell = std::env::var_os("ComSpec").expect("ComSpec");
    let mut child = async_process::Command::new(shell)
        .args(["/d", "/c", line])
        .stdout(async_process::Stdio::null())
        .stderr(async_process::Stdio::null())
        .spawn()
        .expect("start cmd");
    block_on(child.status()).expect("cmd ran");
}

#[test]
fn the_kill_spares_a_browser_and_ends_the_rest() {
    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    assert!(!bin.to_string_lossy().contains(' '), "{}", bin.display());
    let dir = folder("spare-browser");
    let edge = dir.join("msedge.exe");
    cmd(&format!(
        "mkdir {} & copy /y {} {} >nul",
        dir.display(),
        bin.display(),
        edge.display()
    ));
    let shell: PathBuf = std::env::var_os("ComSpec").expect("ComSpec").into();
    let line = format!(
        "start /b {} --hold & {} --hold",
        edge.display(),
        bin.display()
    );
    let spec = LaunchSpec::new(
        shell,
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
    cmd(&format!("rd /s /q {}", dir.display()));
    assert!(rest_gone, "the tree's other processes still run");
    assert!(browser_runs, "the browser was ended with the tree");
}

/// The file name of `path`, as a `&str`.
#[cfg(test)]
fn file_name(path: &Path) -> &str {
    path.file_name().and_then(|name| name.to_str()).unwrap()
}

/// A sign-in's Cancel after its login opened the browser: the run's own processes exit by
/// themselves, and the browser they started, spared, holds the pipes it inherited. The wait
/// for the run and its output (`Handle::exited_draining`) returns well inside the lawyer's 2 s
/// grace, with what the run wrote before it exited, and the browser keeps running.
#[test]
fn the_exit_wait_ends_once_only_a_spared_browser_holds_the_pipes() {
    use futures::io::AsyncReadExt as _;

    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    assert!(!bin.to_string_lossy().contains(' '), "{}", bin.display());
    let dir = folder("drain-browser");
    let edge = dir.join("msedge.exe");
    cmd(&format!(
        "mkdir {} & copy /y {} {} >nul",
        dir.display(),
        bin.display(),
        edge.display()
    ));
    // `start /b` hands the browser cmd's stdout and stderr, the run's pipes; cmd then waits for
    // a line on its stdin, writes one of its own and exits by itself.
    let shell: PathBuf = std::env::var_os("ComSpec").expect("ComSpec").into();
    let line = format!(
        "start /b {} --hold & set /p go= & echo before-exit",
        edge.display()
    );
    let spec = LaunchSpec::new(
        shell,
        vec!["/d".into(), "/c".into(), line.into()],
        BTreeMap::new(),
        env!("CARGO_MANIFEST_DIR").into(),
    )
    .unwrap()
    .spare_browsers();
    let launched = spawn_in_tree(&spec, &SpawnOptions::new()).unwrap();
    let mut handle = launched.handle;
    let mut stdin = launched.stdin;
    let mut stdout = launched.stdout;
    let stderr = launched.stderr;
    let descendants = handle.descendants();
    let mut browser = None;
    let started = within_bound(|| {
        browser = find(
            &descendants.snapshot().pids().unwrap_or_default(),
            "msedge.exe",
        );
        browser.is_some()
    });
    let Some(browser) = browser else {
        panic!("the browser never started: {started}");
    };
    let mut out = Vec::new();
    let finished = block_on(async {
        use futures::io::AsyncWriteExt as _;
        drop(stdin.write_all(b"go\r\n").await);
        drop(stdin.close().await);
        let read = async {
            let mut chunk = [0_u8; 4096];
            while let Ok(n @ 1..) = stdout.read(&mut chunk).await {
                out.extend_from_slice(chunk.get(..n).unwrap_or_default());
            }
        };
        let errs = async {
            drop(stderr.drain().await);
        };
        let pipes = async {
            futures::future::join(read, errs).await;
        };
        let wait = Box::pin(handle.exited_draining(&clauseo_process::AsyncIoTimer, pipes));
        // The lawyer's grace is 2 s, and the bound was what this wait took before.
        let bound = Box::pin(Timer::after(Duration::from_millis(1_500)));
        match futures::future::select(wait, bound).await {
            futures::future::Either::Left(((_report, ended), _)) => Some(ended),
            futures::future::Either::Right(_) => None,
        }
    });
    let browser_runs = running(browser);
    drop(handle);
    kill(browser);
    within_bound(|| !running(browser));
    cmd(&format!("rd /s /q {}", dir.display()));
    let Some(ended) = finished else {
        panic!("the wait ran past 1.5 s");
    };
    assert!(
        !ended,
        "the browser held the pipes, so they never reached their end"
    );
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("before-exit"), "{text}");
    assert!(browser_runs, "the browser was ended with the tree");
}
