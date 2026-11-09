//! The scan's rule against a fake kernel: the mark allowed and the canary denied, sandboxed
//! processes only, this user's only, `since`, a process named by its pid and start, and the drift
//! check on the launch's agent.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};

use super::*;
use crate::test_support::{TempDir, remove};

/// The fake kernel: each sandboxed pid with the paths it may write; a pid not listed is
/// unsandboxed, and may write any path (as the real check answers for an unsandboxed process).
#[derive(Default)]
struct Fake(HashMap<u32, HashSet<CString>>);

impl Fake {
    fn jailed(mut self, pid: u32, writable: &[&CStr]) -> Fake {
        self.0
            .insert(pid, writable.iter().map(|p| (*p).to_owned()).collect());
        self
    }
}

impl Checker for Fake {
    fn sandboxed(&self, pid: u32) -> Option<bool> {
        Some(self.0.contains_key(&pid))
    }

    fn may_write(&self, pid: u32, path: &CStr) -> Option<bool> {
        Some(self.0.get(&pid).is_none_or(|paths| paths.contains(path)))
    }
}

const UID: u32 = 501;
/// "Now" for every scan: 100 s after the epoch.
const NOW: u64 = 100_000_000;

fn proc(pid: u32, start: u64) -> Proc {
    Proc {
        pid,
        parent: 1,
        session: pid,
        start,
        uid: UID,
    }
}

fn pids(found: &[ProcId]) -> Vec<u32> {
    let mut pids: Vec<u32> = found.iter().map(|p| p.pid).collect();
    pids.sort_unstable();
    pids
}

/// Two launches' marks, S and T, in one folder.
fn marks(dir: &TempDir) -> (SessionMark, SessionMark) {
    let s = SessionMark::create(dir.path(), "clauseo-1-0").unwrap();
    let t = SessionMark::create(dir.path(), "clauseo-1-1").unwrap();
    (s, t)
}

#[test]
fn a_process_matches_when_the_mark_is_allowed_and_the_canary_denied() {
    let dir = TempDir::new("sweep-rule");
    let (s, t) = marks(&dir);
    let fake = Fake::default()
        // S's process: S's mark only.
        .jailed(10, &[s.mark_c()])
        // T's process.
        .jailed(11, &[t.mark_c()])
        // An Apple daemon that may write any file in the folder: both marks and both canaries.
        .jailed(12, &[s.mark_c(), s.canary_c(), t.mark_c(), t.canary_c()])
        // A sandboxed process that may write neither.
        .jailed(13, &[]);
    // 14 is unsandboxed: it may write every path, and is not sandboxed.
    let all: Vec<Proc> = (10..=14).map(|pid| proc(pid, 50_000_000)).collect();
    let found = scan(&fake, &[&s], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [10]);
    let found = scan(&fake, &[&t], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [11]);
    let found = scan(&fake, &[&s, &t], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [10, 11]);
}

#[test]
fn another_user_s_process_is_never_asked_about() {
    let dir = TempDir::new("sweep-uid");
    let (s, _) = marks(&dir);
    let fake = Fake::default().jailed(10, &[s.mark_c()]);
    let mut other = proc(10, 50_000_000);
    other.uid = 0;
    assert_eq!(scan(&fake, &[&s], 0, NOW, UID, &[other]).unwrap(), []);
}

#[test]
fn a_process_older_than_since_is_skipped() {
    let dir = TempDir::new("sweep-since");
    let (s, _) = marks(&dir);
    let fake = Fake::default()
        .jailed(10, &[s.mark_c()])
        .jailed(11, &[s.mark_c()]);
    let all = [proc(10, 40_000_000), proc(11, 60_000_000)];
    let found = scan(&fake, &[&s], 50_000_000, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [11]);
    // A start at `since` exactly counts.
    let found = scan(&fake, &[&s], 40_000_000, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [10, 11]);
}

#[test]
fn a_process_is_its_pid_and_its_start() {
    let dir = TempDir::new("sweep-identity");
    let (s, _) = marks(&dir);
    let fake = Fake::default().jailed(10, &[s.mark_c()]);
    let found = scan(&fake, &[&s], 0, NOW, UID, &[proc(10, 50_000_000)]).unwrap();
    assert_eq!(found, [ProcId::new(10, 50_000_000)]);
    assert_ne!(found, [ProcId::new(10, 50_000_001)]);
    assert_eq!(
        found.first().map(ProcId::start),
        Some(UNIX_EPOCH + Duration::from_secs(50))
    );
    // The agent registered as pid 20 at 30 s; pid 20 now names a later, unsandboxed process
    // (a reused pid): it is not the agent, so its not matching is no drift.
    s.add_root(ProcId::new(20, 30_000_000));
    let found = scan(&fake, &[&s], 0, NOW, UID, &[proc(20, 70_000_000)]).unwrap();
    assert_eq!(found, []);
}

#[test]
fn an_agent_that_does_not_match_its_own_mark_is_drift() {
    let dir = TempDir::new("sweep-drift");
    let (s, t) = marks(&dir);
    // The agent, pid 10, started at 50 s: matches its mark, so the scan passes and finds it.
    let fake = Fake::default().jailed(10, &[s.mark_c()]);
    s.add_root(ProcId::new(10, 50_000_000));
    let all = [proc(10, 50_000_000)];
    let found = scan(&fake, &[&s], 60_000_000, NOW, UID, &all).unwrap();
    assert_eq!(
        pids(&found),
        [10],
        "the agent is checked whatever `since` says"
    );
    // The kernel's answers drift: the agent is sandboxed but matches no mark.
    let drifted = Fake::default().jailed(10, &[]);
    let error = scan(&drifted, &[&s], 0, NOW, UID, &all).unwrap_err();
    assert!(
        matches!(&error, SweepError::Drift { tag, pid: 10 } if tag == "clauseo-1-0"),
        "{error:?}"
    );
    // Or answers that nothing is sandboxed at all.
    let blind = Fake::default();
    assert!(matches!(
        scan(&blind, &[&s], 0, NOW, UID, &all),
        Err(SweepError::Drift { .. })
    ));
    // An agent that matches another launch's mark only is drift for its own.
    let crossed = Fake::default().jailed(10, &[t.mark_c()]);
    assert!(matches!(
        scan(&crossed, &[&s, &t], 0, NOW, UID, &all),
        Err(SweepError::Drift { .. })
    ));
    // An agent within its first second may not carry its profile yet: no drift.
    let young = [proc(10, NOW - 500_000)];
    s.add_root(ProcId::new(10, NOW - 500_000));
    assert_eq!(scan(&blind, &[&s], 0, NOW, UID, &young).unwrap(), []);
    // An agent that has exited (not listed) is not checked.
    assert_eq!(scan(&blind, &[&s], 0, NOW, UID, &[]).unwrap(), []);
}

#[test]
fn a_missing_mark_or_canary_fails_the_scan() {
    let dir = TempDir::new("sweep-missing");
    let (s, t) = marks(&dir);
    let fake = Fake::default().jailed(10, &[s.mark_c()]);
    let all = [proc(10, 50_000_000)];
    remove(s.canary());
    let error = scan(&fake, &[&s], 0, NOW, UID, &all).unwrap_err();
    assert!(matches!(&error, SweepError::Mark { path, .. } if path == s.canary()));
    t.remove().unwrap();
    assert!(!t.path().exists() && !t.path().parent().unwrap().exists());
    assert!(matches!(
        scan(&fake, &[&t], 0, NOW, UID, &all),
        Err(SweepError::Mark { .. })
    ));
}

#[test]
fn the_mark_is_made_as_a_private_regular_file_beside_its_canary() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("sweep-create");
    let s = SessionMark::create(dir.path(), "clauseo-1-0").unwrap();
    let folder = dunce::canonicalize(dir.path()).unwrap().join("clauseo-1-0");
    assert_eq!(s.path(), folder.join("mark"));
    assert_eq!(s.canary(), folder.join("canary"));
    assert_eq!(s.param(), ("SESSION_MARK".to_owned(), folder.join("mark")));
    for path in [s.path(), s.canary()] {
        let meta = std::fs::symlink_metadata(path).unwrap();
        assert!(meta.is_file());
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }
    let meta = std::fs::symlink_metadata(&folder).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    // Made again (an earlier host with the same pid): kept.
    SessionMark::create(dir.path(), "clauseo-1-0").unwrap();
    // A tag that is not a launcher's, or a link in the mark's place, is refused.
    assert!(SessionMark::create(dir.path(), "../x").is_err());
    let elsewhere = dir.file("elsewhere.txt", "");
    dir.dir("links/clauseo-1-2");
    dir.symlink("links/clauseo-1-2/mark", &elsewhere);
    assert!(SessionMark::create(&dir.path().join("links"), "clauseo-1-2").is_err());
}

/// A pid that names no process, from the top of the pid space down.
fn dead_pid() -> u32 {
    (90_000..99_000)
        .rev()
        .find(|&pid| !host_alive(pid))
        .expect("a free pid")
}

#[test]
fn a_dropped_mark_no_process_matches_removes_itself() {
    let dir = TempDir::new("sweep-drop");
    let s = SessionMark::create(dir.path(), "clauseo-1-0").unwrap();
    let folder = s.path().parent().unwrap().to_owned();
    drop(s);
    assert!(!folder.exists(), "the unused mark stayed");
    // A mark found on disk is never removed by its drop.
    let mut kept = SessionMark::create(dir.path(), "clauseo-1-1").unwrap();
    kept.clean_on_drop = false;
    let folder = kept.path().parent().unwrap().to_owned();
    drop(kept);
    drop(SessionMark::open(&folder).unwrap());
    assert!(folder.join("mark").exists() && folder.join("canary").exists());
}

#[test]
fn only_an_unused_mark_of_a_host_that_is_gone_is_cleaned() {
    let dir = TempDir::new("sweep-stale");
    let root = dunce::canonicalize(dir.path()).unwrap();
    let left = |tag: &str| {
        let mut mark = SessionMark::create(&root, tag).unwrap();
        mark.clean_on_drop = false;
        mark.path().parent().unwrap().to_owned()
    };
    let dead = left(&format!("clauseo-{}-0", dead_pid()));
    let ours = left(&format!("clauseo-{}-99", std::process::id()));
    // launchd: a host that still runs.
    let running = left("clauseo-1-7");
    // A broken mark (its canary gone) of a dead host: its scan fails, so it stays.
    let broken = left(&format!("clauseo-{}-1", dead_pid()));
    remove(&broken.join("canary"));
    dir.dir("not-a-mark");
    assert_eq!(clean_stale(&root), 1);
    assert!(!dead.exists());
    for kept in [&ours, &running, &broken] {
        assert!(kept.join("mark").exists(), "{}", kept.display());
    }
    assert!(root.join("not-a-mark").exists());
    assert_eq!(host_of("clauseo-4242-7"), Some(4242));
    assert_eq!(host_of("clauseo-x-7"), None);
}
