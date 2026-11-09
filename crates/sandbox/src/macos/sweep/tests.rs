//! The scan's rule against a fake kernel: the mark allowed and the canary denied, sandboxed
//! processes only, this user's only, `since`, a process named by its pid and start, the
//! sentinel that vouches for every scan, the drift check on the launch's agent, and the gate
//! refusing when the kernel's answers break.

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_void};

use super::*;
use crate::test_support::{TempDir, remove};

mod unvouched;

/// The fake kernel: each sandboxed pid with the paths it may write; a pid not listed is
/// unsandboxed, and may write any path (as the real check answers for an unsandboxed process).
/// A pid in its second set has exited since the listing.
#[derive(Default)]
struct Fake(HashMap<u32, HashSet<CString>>, HashSet<u32>);

impl Fake {
    fn jailed(mut self, pid: u32, writable: &[&CStr]) -> Fake {
        self.0
            .insert(pid, writable.iter().map(|p| (*p).to_owned()).collect());
        self
    }

    /// `pid` exits after the listing: the kernel answers for it as for a zombie (sandboxed,
    /// every path denied, measured), and it no longer runs when asked again.
    fn exited(mut self, pid: u32) -> Fake {
        self.0.insert(pid, HashSet::new());
        self.1.insert(pid);
        self
    }
}

impl Checker for Fake {
    fn sandboxed(&self, pid: u32) -> Answer {
        Ok(self.0.contains_key(&pid))
    }

    fn may_write(&self, pid: u32, path: &CStr) -> Answer {
        Ok(self.0.get(&pid).is_none_or(|paths| paths.contains(path)))
    }

    fn still_runs(&self, id: ProcId) -> bool {
        !self.1.contains(&id.pid)
    }
}

/// A kernel whose call fails for every process (it answers -1, as for `EPERM`).
struct Failing;

impl Checker for Failing {
    fn sandboxed(&self, _: u32) -> Answer {
        Err(CheckFailed::Failed)
    }

    fn may_write(&self, _: u32, _: &CStr) -> Answer {
        Err(CheckFailed::Failed)
    }

    fn still_runs(&self, _: ProcId) -> bool {
        true
    }
}

/// A kernel that reads the path filter wrongly (a renumbered `SANDBOX_FILTER_PATH`): every
/// sandboxed process is denied every path.
struct WrongFilter;

impl Checker for WrongFilter {
    fn sandboxed(&self, _: u32) -> Answer {
        Ok(true)
    }

    fn may_write(&self, _: u32, _: &CStr) -> Answer {
        Ok(false)
    }

    fn still_runs(&self, _: ProcId) -> bool {
        true
    }
}

const UID: u32 = 501;
/// "Now" for every scan: 100 s after the epoch.
const NOW: u64 = 100_000_000;
/// The sentinels' pids in the fakes.
const SENTINEL: u32 = 90;

fn proc(pid: u32, start: u64) -> Proc {
    Proc {
        pid,
        parent: 1,
        session: pid,
        start,
        uid: UID,
    }
}

/// [`scan`], answering only a vouched scan's finds.
fn scanned(
    check: &dyn Checker,
    marks: &[&SessionMark],
    since: u64,
    now: u64,
    uid: u32,
    all: &[Proc],
) -> Result<Vec<ProcId>, SweepError> {
    scan(check, marks, since, now, uid, all)?.vouched()
}

fn pids(found: &[ProcId]) -> Vec<u32> {
    let mut pids: Vec<u32> = found.iter().map(|p| p.pid).collect();
    pids.sort_unstable();
    pids
}

/// Removes its marks when the test ends, passed or not: a mark with a stand-in sentinel is
/// never removed by its drop (the real scan cannot find the stand-in), and an immutable mark
/// would keep its test's folder.
struct Removed<'a>(Vec<&'a SessionMark>);

impl Drop for Removed<'_> {
    fn drop(&mut self) {
        for mark in &self.0 {
            if let Err(error) = mark.remove() {
                eprintln!("removing {}: {error}", mark.path().display());
            }
        }
    }
}

/// Two launches' marks, S and T, in one folder.
fn marks(dir: &TempDir) -> (SessionMark, SessionMark) {
    let s = SessionMark::create(dir.path(), "clauseo-1-0").unwrap();
    let t = SessionMark::create(dir.path(), "clauseo-1-1").unwrap();
    (s, t)
}

/// Gives each of `marks` a stand-in sentinel (pids from [`SENTINEL`], started at 1 s), jailed
/// in `fake` with its mark; answers the fake and the sentinels' processes.
fn vouched(mut fake: Fake, marks: &[&SessionMark]) -> (Fake, Vec<Proc>) {
    let mut all = Vec::new();
    for (pid, mark) in (SENTINEL..).zip(marks) {
        mark.stand_in_sentinel(ProcId::new(pid, 1_000_000));
        fake = fake.jailed(pid, &[mark.mark_c()]);
        all.push(proc(pid, 1_000_000));
    }
    (fake, all)
}

#[test]
fn a_process_matches_when_the_mark_is_allowed_and_the_canary_denied() {
    let dir = TempDir::new("sweep-rule");
    let (s, t) = marks(&dir);
    let _removed = Removed(vec![&s, &t]);
    let fake = Fake::default()
        // S's process: S's mark only.
        .jailed(10, &[s.mark_c()])
        // T's process.
        .jailed(11, &[t.mark_c()])
        // An Apple daemon that may write any file in the folder: both marks and both canaries.
        .jailed(12, &[s.mark_c(), s.canary_c(), t.mark_c(), t.canary_c()])
        // A sandboxed process that may write neither.
        .jailed(13, &[]);
    let (fake, mut all) = vouched(fake, &[&s, &t]);
    // 14 is unsandboxed: it may write every path, and is not sandboxed.
    all.extend((10..=14).map(|pid| proc(pid, 50_000_000)));
    let found = scanned(&fake, &[&s], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [10], "the sentinel is never found");
    let found = scanned(&fake, &[&t], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [11]);
    let found = scanned(&fake, &[&s, &t], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [10, 11]);
}

#[test]
fn another_user_s_process_is_never_asked_about() {
    let dir = TempDir::new("sweep-uid");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    let (fake, mut all) = vouched(Fake::default().jailed(10, &[s.mark_c()]), &[&s]);
    let mut other = proc(10, 50_000_000);
    other.uid = 0;
    all.push(other);
    assert_eq!(scanned(&fake, &[&s], 0, NOW, UID, &all).unwrap(), []);
}

#[test]
fn a_process_older_than_since_is_skipped() {
    let dir = TempDir::new("sweep-since");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    let fake = Fake::default()
        .jailed(10, &[s.mark_c()])
        .jailed(11, &[s.mark_c()]);
    // The sentinel started at 1 s, before `since`: it vouches all the same.
    let (fake, mut all) = vouched(fake, &[&s]);
    all.extend([proc(10, 40_000_000), proc(11, 60_000_000)]);
    let found = scanned(&fake, &[&s], 50_000_000, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [11]);
    // A start at `since` exactly counts.
    let found = scanned(&fake, &[&s], 40_000_000, NOW, UID, &all).unwrap();
    assert_eq!(pids(&found), [10, 11]);
}

#[test]
fn a_process_is_its_pid_and_its_start() {
    let dir = TempDir::new("sweep-identity");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    let (fake, sentinels) = vouched(Fake::default().jailed(10, &[s.mark_c()]), &[&s]);
    let all = [sentinels.clone(), vec![proc(10, 50_000_000)]].concat();
    let found = scanned(&fake, &[&s], 0, NOW, UID, &all).unwrap();
    assert_eq!(found, [ProcId::new(10, 50_000_000)]);
    assert_ne!(found, [ProcId::new(10, 50_000_001)]);
    assert_eq!(
        found.first().map(ProcId::start),
        Some(UNIX_EPOCH + Duration::from_secs(50))
    );
    // The agent was pid 20 at 30 s; pid 20 now names a later, unsandboxed process (a reused
    // pid): it is not the agent, so its not matching is no drift.
    assert!(s.claim(ProcId::new(20, 30_000_000)));
    let all = [sentinels, vec![proc(20, 70_000_000)]].concat();
    assert_eq!(scanned(&fake, &[&s], 0, NOW, UID, &all).unwrap(), []);
    // A sentinel whose pid names a later process vouches for nothing: it is gone.
    let reused = [proc(SENTINEL, 2_000_000)];
    assert!(matches!(
        scanned(&fake, &[&s], 0, NOW, UID, &reused),
        Err(SweepError::SentinelGone { .. })
    ));
}

#[test]
fn a_mark_has_one_agent() {
    // R17: one launch per launcher, so one agent per mark.
    let dir = TempDir::new("sweep-one-agent");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    assert_eq!(s.root(), None);
    assert!(s.claim(ProcId::new(10, 50_000_000)));
    assert!(!s.claim(ProcId::new(11, 60_000_000)));
    assert_eq!(s.root(), Some(ProcId::new(10, 50_000_000)));
}

#[test]
fn an_agent_that_does_not_match_its_own_mark_is_drift() {
    let dir = TempDir::new("sweep-drift");
    let (s, t) = marks(&dir);
    let _removed = Removed(vec![&s, &t]);
    // The agent, pid 10, started at 50 s: matches its mark, so the scan passes and finds it.
    assert!(s.claim(ProcId::new(10, 50_000_000)));
    let (fake, sentinels) = vouched(Fake::default().jailed(10, &[s.mark_c()]), &[&s, &t]);
    let all = [sentinels, vec![proc(10, 50_000_000)]].concat();
    let found = scanned(&fake, &[&s], 60_000_000, NOW, UID, &all).unwrap();
    assert_eq!(
        pids(&found),
        [10],
        "the agent is checked whatever `since` says"
    );
    // The kernel's answers drift for the agent alone: sandboxed but matching no mark.
    let (drifted, _) = vouched(Fake::default().jailed(10, &[]), &[&s, &t]);
    let error = scanned(&drifted, &[&s], 0, NOW, UID, &all).unwrap_err();
    assert!(
        matches!(&error, SweepError::Drift { tag, pid: 10 } if tag == "clauseo-1-0"),
        "{error:?}"
    );
    // An agent that matches another launch's mark only is drift for its own.
    let (crossed, _) = vouched(Fake::default().jailed(10, &[t.mark_c()]), &[&s, &t]);
    assert!(matches!(
        scanned(&crossed, &[&s, &t], 0, NOW, UID, &all),
        Err(SweepError::Drift { .. })
    ));
    // An agent within its first second may not carry its profile yet: no drift.
    let young = SessionMark::create(dir.path(), "clauseo-1-2").unwrap();
    let _young = Removed(vec![&young]);
    assert!(young.claim(ProcId::new(10, NOW - 500_000)));
    let (unjailed, sentinels) = vouched(Fake::default(), &[&young]);
    let all = [sentinels.clone(), vec![proc(10, NOW - 500_000)]].concat();
    assert_eq!(
        scanned(&unjailed, &[&young], 0, NOW, UID, &all).unwrap(),
        []
    );
    // An agent that exits between the listing and its check (a zombie by then) is no drift.
    let (gone, sentinels_s) = vouched(Fake::default().exited(10), &[&s]);
    let listed = [sentinels_s, vec![proc(10, 50_000_000)]].concat();
    assert_eq!(scanned(&gone, &[&s], 0, NOW, UID, &listed).unwrap(), []);
    // An agent that has exited (not listed) is not checked.
    assert_eq!(
        scanned(&unjailed, &[&young], 0, NOW, UID, &sentinels).unwrap(),
        []
    );
}

#[test]
fn a_scan_its_sentinel_does_not_vouch_for_is_unknown() {
    // H1: with no agent left there is no drift check; the sentinel is the control that holds.
    let dir = TempDir::new("sweep-sentinel");
    let (s, t) = marks(&dir);
    let _removed = Removed(vec![&s, &t]);
    let unknown = |result: Result<Vec<ProcId>, SweepError>| matches!(result, Err(SweepError::Unknown { tag }) if tag == "clauseo-1-0");
    // No sentinel at all.
    assert!(unknown(scanned(&Fake::default(), &[&s], 0, NOW, UID, &[])));
    let (fake, all) = vouched(Fake::default(), &[&s, &t]);
    assert_eq!(scanned(&fake, &[&s, &t], 0, NOW, UID, &all).unwrap(), []);
    // The sentinel is not running (not listed): killed.
    assert!(matches!(
        scanned(&fake, &[&s], 0, NOW, UID, &[]),
        Err(SweepError::SentinelGone { tag }) if tag == "clauseo-1-0"
    ));
    // The call fails for every process (-1): what would read as "nothing matches".
    assert!(unknown(scanned(&Failing, &[&s], 0, NOW, UID, &all)));
    // The path filter reads wrongly: every path denied, the mark too.
    assert!(unknown(scanned(&WrongFilter, &[&s], 0, NOW, UID, &all)));
    // Nothing is sandboxed any longer.
    let blind = Fake::default();
    assert!(unknown(scanned(&blind, &[&s], 0, NOW, UID, &all)));
    // T's sentinel matching T vouches for T alone.
    let (half, _) = vouched(Fake::default(), &[&t]);
    let all = [proc(SENTINEL, 1_000_000)];
    assert!(scanned(&half, &[&t], 0, NOW, UID, &all).is_ok());
    assert!(unknown(scanned(&half, &[&s, &t], 0, NOW, UID, &all)));
}

#[test]
fn the_gate_refuses_when_the_check_breaks() {
    // H1, through the gate and the real process listing: this test process stands in for the
    // sentinel (it is never signalled), and no agent runs.
    let dir = TempDir::new("sweep-gate");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    let me = std::process::id();
    let start = start_of(me).unwrap();
    s.stand_in_sentinel(ProcId::new(me, start));
    let refused = |check: &dyn Checker| {
        let error = gate_with(check, &[&s], now() + Duration::from_secs(5)).unwrap_err();
        assert!(error.left.is_empty(), "{error:?}");
        matches!(error.why, SweepError::Unknown { .. })
    };
    assert!(refused(&Failing), "a failing call reached Gone");
    assert!(refused(&WrongFilter), "a wrong filter reached Gone");
    assert!(refused(&Fake::default()), "a blind kernel reached Gone");
    // The control: a kernel that answers as measured lets the gate through.
    let working = Fake::default().jailed(me, &[s.mark_c()]);
    let read_only = gate_with(&working, &[&s], now() + Duration::from_secs(5)).unwrap();
    assert_eq!(read_only.gone().ended, 0);
}

#[test]
fn a_missing_mark_or_canary_fails_the_scan() {
    let dir = TempDir::new("sweep-missing");
    let (s, t) = marks(&dir);
    let _removed = Removed(vec![&s, &t]);
    let (fake, mut all) = vouched(Fake::default().jailed(10, &[s.mark_c()]), &[&s, &t]);
    all.push(proc(10, 50_000_000));
    remove(s.canary());
    let error = scanned(&fake, &[&s], 0, NOW, UID, &all).unwrap_err();
    assert!(
        matches!(
            error,
            SweepError::Mark {
                file: "canary",
                kind: std::io::ErrorKind::NotFound
            }
        ),
        "{error:?}"
    );
    // L4: the error names the file by its role, never its path.
    assert!(!error.to_string().contains('/'), "{error}");
    t.remove().unwrap();
    assert!(!t.path().exists() && !t.path().parent().unwrap().exists());
    assert!(matches!(
        scanned(&fake, &[&t], 0, NOW, UID, &all),
        Err(SweepError::Mark { .. })
    ));
    s.remove().unwrap();
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test opens the mark to write and opens a marks folder to its group"
)]
fn the_mark_is_made_as_a_private_immutable_file_beside_its_canary() {
    use std::os::macos::fs::MetadataExt;
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
    // L4: the mark is immutable, so the one write its profile allows writes nothing, not even
    // for its owner; the canary is not.
    let flags = std::fs::symlink_metadata(s.path()).unwrap();
    assert_ne!(flags.st_flags() & libc::UF_IMMUTABLE, 0);
    let write = std::fs::OpenOptions::new().append(true).open(s.path());
    assert_eq!(
        write.unwrap_err().kind(),
        std::io::ErrorKind::PermissionDenied
    );
    // L2: an existing launch folder is refused (every tag has a random nonce), and so is a
    // marks folder its group or others may write.
    assert!(SessionMark::create(dir.path(), "clauseo-1-0").is_err());
    let open = dir.dir("open");
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    let error = SessionMark::create(&open, "clauseo-1-3").unwrap_err();
    assert!(error.to_string().contains("group or by others"), "{error}");
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    SessionMark::create(&open, "clauseo-1-3").unwrap();
    // A tag that is not a launcher's, or a link in the mark's place, is refused.
    assert!(SessionMark::create(dir.path(), "../x").is_err());
    let elsewhere = dir.file("elsewhere.txt", "");
    dir.dir("links");
    dir.symlink("links/clauseo-1-2", elsewhere.parent().unwrap());
    assert!(SessionMark::create(&dir.path().join("links"), "clauseo-1-2").is_err());
    // Removal clears the flag first.
    s.remove().unwrap();
    assert!(!folder.exists());
}

/// A pid that names no process, from the top of the pid space down.
fn dead_pid() -> u32 {
    (90_000..99_000)
        .rev()
        .find(|&pid| start_of(pid).is_none() && Libproc.session_of(pid).is_none())
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
    SessionMark::open(&folder).unwrap().remove().unwrap();
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
    let me = std::process::id();
    let started = start_of(me).unwrap();
    let dead = left(&format!("clauseo-{}-{started}-0-00", dead_pid()));
    // This host: its pid and its start.
    let ours = left(&format!("clauseo-{me}-{started}-99-00"));
    // L2: this host's pid with another start names a host that is gone (a reused pid).
    let reused = left(&format!("clauseo-{me}-{}-5-00", started - 1));
    // A broken mark (its canary gone) of a dead host: its scan fails, so it stays.
    let broken = left(&format!("clauseo-{}-{started}-1-00", dead_pid()));
    remove(&broken.join("canary"));
    dir.dir("not-a-mark");
    // Each dead host's mark is scanned with a control of its own, started for the scan.
    assert_eq!(clean_stale(&root), 2);
    assert!(!dead.exists() && !reused.exists());
    for kept in [&ours, &broken] {
        assert!(kept.join("mark").exists(), "{}", kept.display());
    }
    assert!(root.join("not-a-mark").exists());
    assert_eq!(host_of("clauseo-4242-77-7-00ff"), Some((4242, 77)));
    assert_eq!(host_of("clauseo-x-7"), None);
    assert_eq!(host_of("clauseo-4242"), None);
    SessionMark::open(&ours).unwrap().remove().unwrap();
    let mark = CString::new(broken.join("mark").into_os_string().into_encoded_bytes()).unwrap();
    seatbelt::set_immutable(&mark, false).unwrap();
}

#[test]
fn the_private_call_is_looked_up_and_its_absence_refuses_jailed_launches() {
    // L5: found on this Mac.
    assert!(api().is_ok());
    // A macOS without it, or without its flag: no call, and no launcher.
    let missing = |_: &CStr| std::ptr::null_mut::<c_void>();
    assert!(seatbelt::resolve(missing).is_none());
    let no_flag = |name: &CStr| {
        if name == seatbelt::CHECK_SYMBOL {
            std::ptr::NonNull::<c_void>::dangling().as_ptr()
        } else {
            std::ptr::null_mut()
        }
    };
    assert!(seatbelt::resolve(no_flag).is_none());
    let error = crate::macos::launcher::require_sweep(Err(SweepError::Unavailable)).unwrap_err();
    assert!(
        matches!(&error, crate::Error::SweepUnavailable(_)),
        "{error:?}"
    );
    assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
}
