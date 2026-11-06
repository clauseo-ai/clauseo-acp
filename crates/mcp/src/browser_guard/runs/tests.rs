use std::fs::File;
use std::path::{Path, PathBuf};

use super::{
    Run, SWEEP_LOCK, daemon_port_of, is_id, profile_of, session_of, settled, windows_session,
};
use crate::write;

/// A fresh folder under the temp folder for one test.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("clauseo-mcp-runs-{test}-{}", std::process::id()));
    if dir.exists() {
        write::remove_dir_all(&dir).unwrap();
    }
    write::create_dir_all(&dir).unwrap();
    dir
}

/// A run under `cwd` as a killed guard leaves it: its profile and its lock file, unlocked.
fn stale_run(cwd: &Path, id: &str) -> PathBuf {
    let profile = profile_of(cwd, id);
    write::create_dir_all(&profile.join("Default")).unwrap();
    write::write(&cwd.join(format!("chrome-profile-{id}.lock")), "").unwrap();
    profile
}

fn names(cwd: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(cwd)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_run_has_its_own_id_profile_and_lock_file() {
    let cwd = scratch("claim");
    let run = Run::claim(&cwd).unwrap();
    let other = Run::claim(&cwd).unwrap();
    assert!(is_id(run.id()), "{}", run.id());
    assert!(run.id().starts_with(&format!("{}-", std::process::id())));
    assert_ne!(run.id(), other.id());
    assert_eq!(
        run.profile(),
        cwd.join(format!("chrome-profile-{}", run.id()))
    );
    let lock = format!("chrome-profile-{}.lock", run.id());
    assert!(names(&cwd).contains(&lock));
    // The profile was never made, so the drop deletes the lock file.
    drop(run);
    assert!(!names(&cwd).contains(&lock));
    drop(other);
    assert_eq!(names(&cwd), [SWEEP_LOCK]);
    write::remove_dir_all(&cwd).unwrap();
}

#[test]
fn a_profile_still_there_keeps_its_lock_file_for_the_next_sweep() {
    let cwd = scratch("kept");
    let run = Run::claim(&cwd).unwrap();
    write::create_dir_all(run.profile()).unwrap();
    let id = run.id().to_owned();
    drop(run);
    assert!(names(&cwd).contains(&format!("chrome-profile-{id}.lock")));
    let next = Run::claim(&cwd).unwrap();
    assert!(!profile_of(&cwd, &id).exists());
    assert!(!names(&cwd).contains(&format!("chrome-profile-{id}.lock")));
    drop(next);
    write::remove_dir_all(&cwd).unwrap();
}

/// The sweep deletes a run whose lock nobody holds and a profile with no lock file, and leaves a
/// run whose lock is held (here by this test, standing in for its guard or reaper), whatever its
/// pid: pid 1's run, held, stays.
#[test]
fn the_sweep_deletes_only_runs_nobody_holds() {
    let cwd = scratch("sweep");
    let stale = stale_run(&cwd, "4194304-0000abcd");
    let held = stale_run(&cwd, "1-0000beef");
    let holder = File::open(cwd.join("chrome-profile-1-0000beef.lock")).unwrap();
    holder.lock_shared().unwrap();
    let orphan = profile_of(&cwd, "77-12345678");
    write::create_dir_all(&orphan).unwrap();
    let foreign = cwd.join("chrome-profile");
    write::create_dir_all(&foreign).unwrap();
    let run = Run::claim(&cwd).unwrap();
    assert!(!stale.exists(), "a stale profile is left");
    assert!(!cwd.join("chrome-profile-4194304-0000abcd.lock").exists());
    assert!(!orphan.exists(), "a profile with no lock file is left");
    assert!(held.join("Default").is_dir(), "a held run was swept");
    assert!(cwd.join("chrome-profile-1-0000beef.lock").exists());
    assert!(foreign.is_dir(), "a name the guard does not own was swept");
    drop(run);
    drop(holder);
    let next = Run::claim(&cwd).unwrap();
    assert!(!held.exists(), "a run released is left");
    drop(next);
    write::remove_dir_all(&cwd).unwrap();
}

#[test]
fn only_a_pid_and_8_hex_digits_are_an_id() {
    for id in ["1-00000000", "4294967295-abcdef09"] {
        assert!(is_id(id), "{id}");
    }
    for id in [
        "",
        "1",
        "-0000abcd",
        "12345678901-0000abcd",
        "1-0000abc",
        "1-0000abcde",
        "1-0000ABCD",
        "x-0000abcd",
        "1-0000abcd.lock",
        "1-00/0abcd",
    ] {
        assert!(!is_id(id), "{id}");
    }
    assert_eq!(session_of("1-0000abcd"), "run-1-0000abcd");
}

/// A live run is unsettled, with its profile or before it makes one; a finished run (its drop
/// deleted the lock file) is settled. A free lock with the profile still there is not: that
/// run's cleanup has not finished. The sweep lock and other names do not count.
#[test]
fn runs_settle_once_their_cleanup_has_finished() {
    let cwd = scratch("settled");
    assert!(settled(&cwd), "an empty folder");
    let run = Run::claim(&cwd).unwrap();
    assert!(!settled(&cwd), "a live run before its profile");
    write::create_dir_all(run.profile()).unwrap();
    assert!(!settled(&cwd), "a live run with its profile");
    write::remove_dir_all(run.profile()).unwrap();
    drop(run);
    write::create_dir_all(&cwd.join("chrome-profile")).unwrap();
    assert!(
        settled(&cwd),
        "a finished run, the sweep lock and a foreign name"
    );
    stale_run(&cwd, "4194304-0000abcd");
    assert!(!settled(&cwd), "a free lock with its profile still there");
    write::remove_dir_all(&profile_of(&cwd, "4194304-0000abcd")).unwrap();
    assert!(settled(&cwd), "a free lock file left alone");
    write::remove_dir_all(&cwd).unwrap();
}

/// agent-browser's own vectors (`connection.rs:1884-1891`), and a Windows run name per run
/// whose derived daemon port is the claimed one.
#[test]
fn a_windows_run_name_derives_the_claimed_daemon_port() {
    assert_eq!(daemon_port_of("default"), 50838);
    assert_eq!(daemon_port_of("my-session"), 63105);
    assert_eq!(daemon_port_of("work"), 51184);
    assert_eq!(daemon_port_of(""), 49152);
    for port in [49_152, 50_838, 61_102, 65_534] {
        let one = windows_session("1-0000abcd", port).unwrap();
        let two = windows_session("2-0000abcd", port).unwrap();
        assert!(one.starts_with("run-1-0000abcd-"), "{one}");
        assert_ne!(one, two);
        assert_eq!(daemon_port_of(&one), port);
        assert_eq!(daemon_port_of(&two), port);
        assert!(
            one.len() <= 64 && crate::browser_guard::env::is_session(std::ffi::OsStr::new(&one))
        );
    }
}
