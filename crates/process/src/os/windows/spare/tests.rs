//! The kill that spares a browser, on real processes: `cmd` starting a copy of `fake_agent`
//! named `msedge.exe` and `fake_agent` itself, both holding (`--hold`). The copy and the
//! folder are made and removed by `cmd`, so no file call of the test's own changes a file.
//! Waits poll with a bound or wait on a process handle, never sleep as a sync.

use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_process::{Child, Command, Stdio};
use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::AssignProcessToJobObject;
use windows::Win32::System::Threading::{
    PROCESS_SET_QUOTA, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
    WaitForSingleObject,
};

use super::super::{Root, Tree, open, prepare};
use super::{BEFORE_LIMITS, REFUSE_END, kill_sparing};
use crate::SpawnOptions;

/// How long a process may take to exit once ended, in milliseconds.
const EXIT_WAIT_MS: u32 = 10_000;

fn fake() -> PathBuf {
    let bin = clauseo_fake_agent::bin_path().expect(
        "fake_agent binary; `-p clauseo-process` does not build it: run `cargo build -p clauseo-fake_agent` first",
    );
    assert!(!bin.to_string_lossy().contains(' '), "{}", bin.display());
    bin
}

/// A folder name of the test's own under the temp folder, with no space (the `cmd` lines are
/// not quoted). Made by the first `cmd` line that names it.
fn folder(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("clauseo-{tag}-{}", std::process::id()));
    assert!(!dir.to_string_lossy().contains(' '), "{}", dir.display());
    dir
}

/// Starts `program args` as an agent starts: suspended, then in a new job that spares browsers.
fn start(program: &Path, args: &[&str]) -> (Child, Tree) {
    let mut command = std::process::Command::new(program);
    command.args(args);
    prepare(&mut command, &SpawnOptions::new());
    let mut child = Command::from(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("start the tree");
    let mut tree = Tree::adopt(&mut child).expect("adopt");
    tree.spare_browsers(child.id());
    (child, tree)
}

/// Runs a `cmd /d /c line` outside any job of the test's and waits for it.
fn cmd(line: &str) {
    let shell = std::env::var_os("ComSpec").expect("ComSpec");
    let mut child = Command::new(shell)
        .args(["/d", "/c", line])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start cmd");
    async_io::block_on(child.status()).expect("cmd ran");
}

/// The tree's processes named `name`, once one runs (bounded).
fn wait_for(tree: &Tree, names: &[&str]) -> Vec<u32> {
    let root = Root {
        job: Arc::downgrade(&tree.job),
        pid: 0,
    };
    for _ in 0..500 {
        let listed = root.list().unwrap_or_default();
        let found: Vec<u32> = names
            .iter()
            .filter_map(|name| {
                listed.iter().find_map(|proc| {
                    let file = proc.exe.as_deref()?.file_name()?.to_str()?;
                    file.eq_ignore_ascii_case(name).then_some(proc.pid)
                })
            })
            .collect();
        if found.len() == names.len() {
            return found;
        }
        async_io::block_on(async_io::Timer::after(Duration::from_millis(20)));
    }
    panic!("the tree never held {names:?}");
}

/// Whether `pid` has exited, or does inside the bound.
fn exits(pid: u32) -> bool {
    open(pid, PROCESS_SYNCHRONIZE).is_none_or(|handle| {
        // SAFETY: a live process handle opened with PROCESS_SYNCHRONIZE.
        let waited = unsafe { WaitForSingleObject(raw(&handle), EXIT_WAIT_MS) };
        waited == WAIT_OBJECT_0
    })
}

/// Ends `pid`, for clean-up.
fn end(pid: u32) {
    if let Some(handle) = open(pid, PROCESS_TERMINATE) {
        // SAFETY: a live process handle opened with PROCESS_TERMINATE.
        unsafe { TerminateProcess(raw(&handle), 1) }.unwrap_or_default();
    }
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

/// `cmd` starting the browser stand-in and `fake_agent`, with its folder.
fn browser_tree(tag: &str) -> (Child, Tree, u32, u32, PathBuf) {
    let bin = fake();
    let dir = folder(tag);
    let edge = dir.join("msedge.exe");
    let line = format!(
        "mkdir {dir} & copy /y {bin} {edge} >nul & start /b {edge} --hold & {bin} --hold",
        dir = dir.display(),
        bin = bin.display(),
        edge = edge.display(),
    );
    let shell: PathBuf = std::env::var_os("ComSpec").expect("ComSpec").into();
    let (child, tree) = start(&shell, &["/d", "/c", &line]);
    let file = bin.file_name().unwrap().to_str().unwrap().to_owned();
    let found = wait_for(&tree, &["msedge.exe", &file]);
    (child, tree, found[0], found[1], dir)
}

#[test]
fn the_browser_outlives_the_kill_and_the_job_close() {
    let (child, tree, browser, other, dir) = browser_tree("spare-kept");
    let root = child.id();
    assert!(tree.kill());
    drop(tree);
    let rest_gone = exits(other) && exits(root);
    let browser_runs = open(browser, PROCESS_SYNCHRONIZE).is_some_and(|h| super::alive(&h));
    end(browser);
    assert!(exits(browser));
    cmd(&format!("rd /s /q {}", dir.display()));
    assert!(rest_gone, "the tree's other processes still run");
    assert!(browser_runs, "the browser was ended with the tree");
}

/// A run whose own process is named like a browser is never spared: the whole tree ends.
#[test]
fn the_root_is_never_spared() {
    let dir = folder("spare-root");
    let edge = dir.join("msedge.exe");
    cmd(&format!(
        "mkdir {} & copy /y {} {} >nul",
        dir.display(),
        fake().display(),
        edge.display()
    ));
    let (child, tree) = start(&edge, &["--hold"]);
    assert_eq!(kill_sparing(&tree), None, "the root was spared");
    assert!(tree.kill());
    let gone = exits(child.id());
    drop(tree);
    cmd(&format!("rd /s /q {}", dir.display()));
    assert!(gone, "the root still runs");
}

/// A process the kill cannot end: it ends the whole job, the browser too, never leaving a
/// process behind in a job that no longer kills on close.
#[test]
fn a_process_that_cannot_be_ended_ends_the_whole_job() {
    let (child, tree, browser, other, dir) = browser_tree("spare-refused");
    REFUSE_END.with(|refuse| refuse.set(true));
    assert_eq!(kill_sparing(&tree), None);
    // The tree's kill, whose sparing gives up the same way, then ends the whole job.
    assert!(tree.kill());
    REFUSE_END.with(|refuse| refuse.set(false));
    let all_gone = exits(other) && exits(browser) && exits(child.id());
    drop(tree);
    cmd(&format!("rd /s /q {}", dir.display()));
    assert!(all_gone, "a process of the job still runs");
}

/// A process that joins the job after the last listing moves the job's total: kill-on-close
/// is put back and the whole job ends, the new process with it.
#[test]
fn a_process_joining_after_the_listing_ends_the_whole_job() {
    let (child, tree, browser, other, dir) = browser_tree("spare-joined");
    let job = Arc::clone(&tree.job);
    let late: Arc<Mutex<Option<Child>>> = Arc::default();
    let keep = Arc::clone(&late);
    BEFORE_LIMITS.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            let shell = std::env::var_os("ComSpec").expect("ComSpec");
            let joined = Command::new(shell)
                .args(["/d", "/k"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start cmd");
            assign(&job, joined.id());
            *keep.lock().unwrap() = Some(joined);
        }));
    });
    assert!(tree.kill());
    let joined = late
        .lock()
        .unwrap()
        .as_ref()
        .map(Child::id)
        .expect("the hook ran");
    let all_gone = exits(joined) && exits(other) && exits(browser) && exits(child.id());
    drop(tree);
    cmd(&format!("rd /s /q {}", dir.display()));
    assert!(all_gone, "a process of the job still runs");
}

/// Puts `pid` in `job`.
fn assign(job: &OwnedHandle, pid: u32) {
    let process = open(pid, PROCESS_SET_QUOTA | PROCESS_TERMINATE).expect("open cmd");
    // SAFETY: both are live handles; the process one has the rights the call needs.
    unsafe { AssignProcessToJobObject(raw(job), raw(&process)) }.expect("assign cmd");
}
