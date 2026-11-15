//! The daemon reap, with a stand-in daemon: this test binary itself, started as [`listener`],
//! which leaves the test's session (`setsid`) and listens on a socket as agent-browser's daemon
//! does. The program the reap accepts is given as this binary, the tests' seam; no Chrome.

use std::io::{BufRead as _, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::super::{BrowserSocketDir, LOCK_SUFFIX, PREFIX};
use super::{DaemonProgram, Reap, Reaped, reap_folder};
use crate::browser_dir::Swept;
use crate::test_support::{TempDir, bind, set_mode, spawn};

/// The variable that makes [`listener`] listen, on the socket path it holds.
const LISTEN: &str = "CLAUSEO_REAP_TEST_LISTEN";
/// What [`listener`] prints once it listens.
const READY: &str = "listening";
/// [`listener`]'s full test name.
const LISTENER: &str = "browser_dir::reap::tests::listener";
/// The most a test waits for a stand-in to exit.
const EXIT_WAIT: Duration = Duration::from_secs(5);

/// The stand-in daemon, when [`LISTEN`] is set: a session of its own (`setsid`, as
/// agent-browser's daemon), a listener on the socket, and every connection held open.
#[test]
#[ignore = "the reap tests' stand-in daemon, started as a process of its own; idle when run alone"]
fn listener() {
    let Some(path) = std::env::var_os(LISTEN) else {
        return;
    };
    let _session = rustix::process::setsid();
    let listener = bind(Path::new(&path));
    println!("{READY}");
    let mut held = Vec::new();
    for stream in listener.incoming().flatten() {
        held.push(stream);
    }
}

/// A stand-in daemon listening on `socket` once this returns.
fn start_daemon(socket: &Path) -> Child {
    let exe = std::env::current_exe().unwrap();
    let mut child = spawn(
        Command::new(exe)
            .args([
                LISTENER,
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(LISTEN, socket)
            .stdin(Stdio::null())
            .stdout(Stdio::piped()),
    );
    let stdout = child.stdout.take().unwrap();
    let ready = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.trim_end().ends_with(READY));
    assert!(ready, "the stand-in daemon did not start");
    child
}

/// Waits a moment, without `thread::sleep`.
fn pause(wait: Duration) {
    let timeout = rustix::event::Timespec::try_from(wait).unwrap();
    let _woke = rustix::event::poll(&mut [], Some(&timeout));
}

/// Whether `child` exits within [`EXIT_WAIT`].
fn exits(child: &mut Child) -> bool {
    let step = Duration::from_millis(20);
    for _ in 0..EXIT_WAIT.as_millis() / step.as_millis() {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        pause(step);
    }
    false
}

/// Ends a stand-in a test left running.
fn end(mut child: Child) {
    child.kill().unwrap();
    child.wait().unwrap();
}

/// This test binary as the program the reap accepts: the tests' seam.
fn this_program() -> DaemonProgram {
    DaemonProgram::new(&std::env::current_exe().unwrap()).unwrap()
}

/// Plenty of time, as a caller's budget that never runs out.
fn plenty() -> Duration {
    Duration::from_secs(2)
}

#[test]
fn dropping_the_folder_ends_its_daemon_first() {
    let tmp = TempDir::short("rp-drop");
    let mut dir = BrowserSocketDir::create_in(tmp.path()).unwrap();
    let path = dir.path().to_owned();
    let mut daemon = start_daemon(&path.join("s.sock"));
    dir.reap_on_drop(this_program());
    drop(dir);
    assert!(exits(&mut daemon), "the daemon outlived the folder's drop");
    assert!(
        std::fs::symlink_metadata(&path).is_err(),
        "{}",
        path.display()
    );
}

#[test]
fn a_listener_of_another_program_is_left_alone() {
    let tmp = TempDir::short("rp-other");
    let other = DaemonProgram::new(&tmp.file("other-program", "")).unwrap();
    let mut dir = BrowserSocketDir::create_in(tmp.path()).unwrap();
    let mut daemon = start_daemon(&dir.path().join("s.sock"));
    let left = plenty;
    let reaped = reap_folder(dir.path(), &Reap::new(&other, &left));
    assert_eq!(
        reaped,
        Reaped {
            refused: 1,
            ..Reaped::default()
        }
    );
    dir.reap_on_drop(other);
    drop(dir);
    pause(Duration::from_millis(200));
    assert!(
        daemon.try_wait().unwrap().is_none(),
        "a foreign listener was ended"
    );
    end(daemon);
}

#[test]
fn a_socket_nobody_listens_on_and_other_files_are_passed_over() {
    let tmp = TempDir::short("rp-none");
    let dir = BrowserSocketDir::create_in(tmp.path()).unwrap();
    drop(bind(&dir.path().join("gone.sock")));
    tmp.file(
        &format!("{PREFIX}{}/d.sock", dir.id()),
        "a file named like a socket\n",
    );
    let program = this_program();
    let left = plenty;
    assert_eq!(
        reap_folder(dir.path(), &Reap::new(&program, &left)),
        Reaped::default()
    );
}

/// A folder as a crashed host leaves it: ours, 0700, with its lock file and nobody holding it.
fn stale_folder(tmp: &TempDir, id: &str) -> std::path::PathBuf {
    let folder = tmp.dir(&format!("{PREFIX}{id}"));
    set_mode(&folder, 0o700);
    tmp.file(&format!("{PREFIX}{id}{LOCK_SUFFIX}"), "");
    folder
}

#[test]
fn the_start_sweep_ends_a_stale_folders_daemon_then_removes_the_folder() {
    let tmp = TempDir::short("rp-sweep");
    let folder = stale_folder(&tmp, "aaaaaaaaaaaaaaaa");
    let mut daemon = start_daemon(&folder.join("s.sock"));
    let program = this_program();
    let left = plenty;
    let swept = BrowserSocketDir::sweep_stale_reaping_in(
        tmp.path(),
        |_| false,
        &Reap::new(&program, &left),
    )
    .unwrap();
    let ended = Reaped {
        ended: 1,
        ..Reaped::default()
    };
    assert_eq!(
        swept,
        Swept {
            folders: 1,
            daemons: ended
        }
    );
    assert!(exits(&mut daemon), "the sweep left the daemon");
    assert!(std::fs::symlink_metadata(&folder).is_err());
}

#[test]
fn a_sweep_out_of_time_keeps_the_folder_and_its_daemon_for_the_next_start() {
    let tmp = TempDir::short("rp-late");
    let folder = stale_folder(&tmp, "bbbbbbbbbbbbbbbb");
    let mut daemon = start_daemon(&folder.join("s.sock"));
    let program = this_program();
    let none = || Duration::ZERO;
    let swept = BrowserSocketDir::sweep_stale_reaping_in(
        tmp.path(),
        |_| false,
        &Reap::new(&program, &none),
    )
    .unwrap();
    let left = Reaped {
        left: 1,
        ..Reaped::default()
    };
    assert_eq!(
        swept,
        Swept {
            folders: 0,
            daemons: left
        }
    );
    assert!(std::fs::symlink_metadata(&folder).is_ok());
    assert!(daemon.try_wait().unwrap().is_none());
    end(daemon);
}

#[test]
fn a_live_hosts_folder_is_not_reaped_by_a_sweep() {
    let tmp = TempDir::short("rp-live");
    let live = BrowserSocketDir::create_in(tmp.path()).unwrap();
    let mut daemon = start_daemon(&live.path().join("s.sock"));
    let program = this_program();
    let left = plenty;
    let swept = BrowserSocketDir::sweep_stale_reaping_in(
        tmp.path(),
        |_| false,
        &Reap::new(&program, &left),
    )
    .unwrap();
    assert_eq!(swept, Swept::default());
    assert!(daemon.try_wait().unwrap().is_none());
    end(daemon);
    drop(live);
}
