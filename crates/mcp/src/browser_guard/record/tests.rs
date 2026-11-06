use std::path::{Path, PathBuf};

#[cfg(unix)]
use super::forget;
use super::{ChromeProcess, ChromeState, chrome_groups, parse, write_state};
use crate::browser_guard::runs::{Run, profile_of, record_of};
use crate::write;

fn scratch(test: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("clauseo-mcp-record-{test}-{}", std::process::id()));
    if dir.exists() {
        write::remove_dir_all(&dir).unwrap();
    }
    write::create_dir_all(&dir).unwrap();
    dir
}

/// A process in a group of its own that waits to be killed: a shell running `sleep`, with
/// `args` on its command line.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the test needs a live process group to probe"
)]
fn group(args: &[&std::ffi::OsStr]) -> std::process::Child {
    use std::os::unix::process::CommandExt;
    std::process::Command::new("sh")
        .args(["-c", "sleep 30; :", "sh"])
        .args(args)
        .process_group(0)
        .spawn()
        .unwrap()
}

#[test]
fn records_parse_and_anything_else_reads_as_starting() {
    assert_eq!(parse("none"), ChromeState::None);
    assert_eq!(parse("starting"), ChromeState::Starting);
    let unix = ChromeProcess {
        id: 4100,
        created: 0,
    };
    assert_eq!(parse("running 4100\n"), ChromeState::Running(unix));
    let windows = ChromeProcess {
        id: 4100,
        created: 133_000_000_000_000_000,
    };
    assert_eq!(
        parse("running 4100 133000000000000000"),
        ChromeState::Running(windows)
    );
    for text in [
        "",
        "nothing",
        "running",
        "running x",
        "running 1 2 3",
        "none 1",
        "-1",
    ] {
        assert_eq!(parse(text), ChromeState::Starting, "{text:?}");
    }
}

/// A live group is not gone; after its kill (and reap) it is.
#[cfg(unix)]
#[test]
fn a_group_is_gone_only_once_killed() {
    let mut child = group(&[]);
    let process = ChromeProcess {
        id: child.id(),
        created: 0,
    };
    assert!(!process.gone(), "a live group read as gone");
    crate::browser_guard::chrome::kill_group(child.id());
    drop(child.wait());
    assert!(process.gone(), "a killed group read as alive");
}

fn state_of(cwd: &Path) -> Vec<ChromeState> {
    chrome_groups(cwd)
}

/// One run at a time: what each record and run says. A live run with no record yet has no
/// Chrome; `starting` and a free run with a profile left and no record are unknown, so
/// `Starting`; a settled run whose record says `none` is not listed; a record left saying
/// `starting` or naming a process is listed, settled or not.
#[test]
fn each_run_reads_as_its_record_says() {
    let cwd = scratch("states");
    assert_eq!(state_of(&cwd), []);
    let run = Run::claim(&cwd).unwrap();
    assert_eq!(
        state_of(&cwd),
        [ChromeState::None],
        "a live run before its record"
    );
    write_state(&cwd, run.id(), &ChromeState::Starting);
    assert_eq!(state_of(&cwd), [ChromeState::Starting]);
    let process = ChromeProcess {
        id: 4100,
        created: 0,
    };
    write_state(&cwd, run.id(), &ChromeState::Running(process));
    assert_eq!(state_of(&cwd), [ChromeState::Running(process)]);
    write_state(&cwd, run.id(), &ChromeState::None);
    assert_eq!(state_of(&cwd), [ChromeState::None]);
    let id = run.id().to_owned();
    drop(run);
    assert_eq!(state_of(&cwd), [], "a finished run whose record says none");
    write_state(&cwd, &id, &ChromeState::Starting);
    assert_eq!(
        state_of(&cwd),
        [ChromeState::Starting],
        "a record left saying starting"
    );
    write::remove_file(&record_of(&cwd, &id)).unwrap();
    write::create_dir_all(&profile_of(&cwd, &id)).unwrap();
    assert_eq!(
        state_of(&cwd),
        [ChromeState::Starting],
        "a profile left with no record"
    );
    write::remove_dir_all(&cwd).unwrap();
}

/// The reaper's and the sweep's rule: a record goes only when it names no Chrome that may live.
/// `starting` with nothing running with the run's profile goes at once.
#[cfg(unix)]
#[test]
fn a_record_is_forgotten_only_when_no_chrome_may_live() {
    let cwd = scratch("forget");
    let id = "77-12345678";
    write_state(&cwd, id, &ChromeState::Starting);
    forget(&cwd, id);
    assert!(
        !record_of(&cwd, id).exists(),
        "starting with no process left was kept"
    );
    let mut child = group(&[]);
    let process = ChromeProcess {
        id: child.id(),
        created: 0,
    };
    write_state(&cwd, id, &ChromeState::Running(process));
    forget(&cwd, id);
    assert!(record_of(&cwd, id).exists(), "a live group was forgotten");
    crate::browser_guard::chrome::kill_group(child.id());
    drop(child.wait());
    forget(&cwd, id);
    assert!(!record_of(&cwd, id).exists(), "a gone group was kept");
    write_state(&cwd, id, &ChromeState::None);
    forget(&cwd, id);
    assert!(!record_of(&cwd, id).exists(), "none was kept");
    write::remove_dir_all(&cwd).unwrap();
}

/// A record left saying `starting` while a process runs with the run's profile (a guard killed
/// after Chrome's spawn, before its pid was told): `forget` kills that process's group, then
/// deletes the record. A process with another run's profile is left alone.
#[cfg(unix)]
#[test]
fn a_starting_record_ends_what_runs_with_the_profile_then_goes() {
    use std::ffi::OsString;
    use std::os::unix::process::ExitStatusExt;
    let cwd = scratch("forget_starting");
    let (id, other) = ("78-12345678", "78-87654321");
    let flag = |id: &str| {
        let mut flag = OsString::from("--user-data-dir=");
        flag.push(profile_of(&cwd, id));
        flag
    };
    let mut bystander = group(&[&flag(other)]);
    let chrome = group(&[&flag(id)]);
    // Reaped as soon as it is killed, as Chrome's new parent would.
    let waiter = std::thread::spawn(move || {
        let mut chrome = chrome;
        chrome.wait().unwrap()
    });
    write_state(&cwd, id, &ChromeState::Starting);
    forget(&cwd, id);
    assert_eq!(waiter.join().unwrap().signal(), Some(9), "not killed");
    assert!(!record_of(&cwd, id).exists(), "the record was kept");
    assert!(
        bystander.try_wait().unwrap().is_none(),
        "another run's process was killed"
    );
    crate::browser_guard::chrome::kill_group(bystander.id());
    drop(bystander.wait());
    write::remove_dir_all(&cwd).unwrap();
}
