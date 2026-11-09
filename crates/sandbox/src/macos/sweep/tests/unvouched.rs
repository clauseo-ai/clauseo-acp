//! Unvouched scans: what they found still counts and is ended, a failed call is unknown, and a
//! dead sentinel is restarted until it is ended for good.

use super::*;

/// [`Fake`], but the call fails for one pid, with `ESRCH` (`Gone`) or another `errno`.
struct FailsFor(Fake, u32, CheckFailed);

impl Checker for FailsFor {
    fn sandboxed(&self, pid: u32) -> Answer {
        if pid == self.1 {
            return Err(self.2);
        }
        self.0.sandboxed(pid)
    }

    fn may_write(&self, pid: u32, path: &CStr) -> Answer {
        if pid == self.1 {
            return Err(self.2);
        }
        self.0.may_write(pid, path)
    }
}

#[test]
fn an_unvouched_scan_still_names_what_it_found() {
    // A match is positive evidence even when the sentinel cannot vouch for the rest.
    let dir = TempDir::new("sweep-unvouched");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    let (fake, sentinels) = vouched(Fake::default().jailed(10, &[s.mark_c()]), &[&s]);
    let escapee = proc(10, 50_000_000);
    // The sentinel was killed (it is not listed): found, with the killing as the doubt.
    let killed = scan(&fake, &[&s], 0, NOW, UID, &[escapee]).unwrap();
    assert_eq!(pids(&killed.found), [10]);
    assert!(
        matches!(&killed.doubt, Some(SweepError::SentinelGone { tag }) if tag == "clauseo-1-0"),
        "{killed:?}"
    );
    // The sentinel runs but no longer matches: found, unknown.
    let unmatched = Fake::default().jailed(10, &[s.mark_c()]);
    let all = [sentinels.clone(), vec![escapee]].concat();
    let scanned = scan(&unmatched, &[&s], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&scanned.found), [10]);
    assert!(matches!(scanned.doubt, Some(SweepError::Unknown { .. })));
    // The agent drifts: what matched is found all the same.
    assert!(s.claim(ProcId::new(11, 40_000_000)));
    let all = [sentinels, vec![escapee, proc(11, 40_000_000)]].concat();
    let drifted = scan(&fake, &[&s], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&drifted.found), [10]);
    assert!(matches!(
        drifted.doubt,
        Some(SweepError::Drift { pid: 11, .. })
    ));
    // A mark with no sentinel at all is unknown, not killed.
    let none = SessionMark::create(dir.path(), "clauseo-1-5").unwrap();
    let _none = Removed(vec![&none]);
    let bare = scan(&fake, &[&none], 0, NOW, UID, &all).unwrap();
    assert!(matches!(bare.doubt, Some(SweepError::Unknown { .. })));
}

#[test]
fn a_failed_check_is_unknown_and_a_process_gone_since_the_listing_is_not() {
    // L1: -1 with ESRCH is a process that exited since the listing; any other errno is a call
    // that no longer answers, so the scan is unknown, never "no match".
    let dir = TempDir::new("sweep-errno");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    let (fake, sentinels) = vouched(Fake::default().jailed(10, &[s.mark_c()]), &[&s]);
    let all = [sentinels, vec![proc(10, 50_000_000), proc(13, 50_000_000)]].concat();
    let gone = FailsFor(fake, 13, CheckFailed::Gone);
    assert_eq!(
        pids(&scanned(&gone, &[&s], 0, NOW, UID, &all).unwrap()),
        [10]
    );
    let failed = FailsFor(gone.0, 13, CheckFailed::Failed);
    let result = scan(&failed, &[&s], 0, NOW, UID, &all).unwrap();
    assert_eq!(pids(&result.found), [10]);
    assert!(
        matches!(result.doubt, Some(SweepError::Unknown { .. })),
        "{result:?}"
    );
}

#[test]
fn an_unvouched_gate_still_ends_what_matched() {
    // (a): the sentinel cannot vouch, so the gate refuses, but what matched is ended.
    use clauseo_process::{LaunchSpec, Launcher, PassthroughLauncher, SpawnOptions};
    let dir = TempDir::new("sweep-gate-kills");
    let (s, _) = marks(&dir);
    let _removed = Removed(vec![&s]);
    let me = std::process::id();
    s.stand_in_sentinel(ProcId::new(me, start_of(me).unwrap()));
    let spec = LaunchSpec::new(
        "/bin/sleep".into(),
        vec!["30".into()],
        std::collections::BTreeMap::new(),
        "/".into(),
    )
    .unwrap();
    let mut escapee = PassthroughLauncher::new(SpawnOptions::new())
        .launch(&spec)
        .unwrap();
    // The fake kernel jails the escapee with the mark, but not the sentinel.
    let fake = Fake::default().jailed(escapee.handle.pid(), &[s.mark_c()]);
    let refused = gate_with(&fake, &[&s], now() + Duration::from_secs(5)).unwrap_err();
    assert!(
        matches!(refused.why, SweepError::Unknown { .. }),
        "{refused:?}"
    );
    assert!(refused.left.is_empty(), "{refused:?}");
    let report = async_io::block_on(escapee.handle.exited());
    assert_eq!(report.signal(), Some(libc::SIGKILL), "{report:?}");
}

#[test]
fn a_dead_sentinel_is_restarted_until_it_is_ended() {
    // (b): a fresh process under the same profile is as good a control.
    let dir = TempDir::new("sweep-revive");
    let s = SessionMark::create(dir.path(), "clauseo-1-0").unwrap();
    let _removed = Removed(vec![&s]);
    let check = api().unwrap();
    let first = s.start_sentinel(check, &s.control_args()).unwrap();
    assert!(!s.revive(check), "a running sentinel was restarted");
    assert!(Libproc.signal(first.pid(), first.start, libc::SIGKILL));
    // Killed, it is a zombie (unreaped), which no listing shows, within a moment.
    let deadline = now() + Duration::from_secs(2);
    let mut killed = scan_with(check, &[&s], UNIX_EPOCH).unwrap();
    while !matches!(killed.doubt, Some(SweepError::SentinelGone { .. })) && now() < deadline {
        std::thread::park_timeout(END_PAUSE);
        killed = scan_with(check, &[&s], UNIX_EPOCH).unwrap();
    }
    assert!(
        matches!(killed.doubt, Some(SweepError::SentinelGone { .. })),
        "{killed:?}"
    );
    assert!(s.revive(check));
    let second = s.sentinel().unwrap();
    assert_ne!(second, first);
    assert_eq!(processes(&[&s], UNIX_EPOCH).unwrap(), []);
    // Ended for good, it is never restarted, and the scan is unknown.
    s.end_sentinel();
    assert!(!s.revive(check));
    assert_eq!(s.sentinel(), None);
    assert!(matches!(
        processes(&[&s], UNIX_EPOCH),
        Err(SweepError::Unknown { .. })
    ));
}
