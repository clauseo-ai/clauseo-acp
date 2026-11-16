//! The refusals before the walk, the BITS step, and the exit codes.

use super::*;

#[test]
fn only_the_pools_bits_jobs_are_cancelled_and_a_job_left_is_56() {
    let mut fake = Fake::new(&[1010, 1011]);
    let owner = Some(Sid::parse(OWNER).unwrap());
    fake.jobs = Ok(vec![
        owner,
        Some(sid(1010)),
        Some(sid(1013)),
        Some(sid(1011)),
    ]);
    assert_eq!(sweep(&mut fake), SweepExit::Done);
    assert_eq!(fake.cancelled(), [1, 3]);
    assert!(
        fake.logged[0].1.ends_with("2 BITS jobs cancelled, 0 not"),
        "{:?}",
        fake.logged
    );
    // A cancel that fails, an owner that can't be read, a list that can't be read or is cut.
    let cases: [(Listed, Vec<usize>); 4] = [
        (Ok(vec![Some(sid(1010)), Some(sid(1011))]), vec![0]),
        (Ok(vec![None, Some(sid(1011))]), Vec::new()),
        (
            Err("CoCreateInstance(BackgroundCopyManager) failed".to_owned()),
            Vec::new(),
        ),
        (Ok(vec![Some(sid(1013)); JOBS_CAP]), Vec::new()),
    ];
    for (jobs, refused) in cases {
        let mut fake = Fake::new(&[1010, 1011]);
        fake.jobs = jobs;
        fake.jobs_refuse = refused;
        assert_eq!(sweep(&mut fake), SweepExit::JobsLeft);
        assert!(fake.logged[0].0);
        assert_eq!(fake.deleted(), Vec::<&str>::new());
    }
    assert_eq!(SweepExit::JobsLeft.as_code(), 57);
    // A task left and a job left: 56 wins, and the job of the pool's is still cancelled.
    let mut fake = Fake::new(&[1010]);
    fake.unread("\\", "unread");
    fake.jobs = Ok(vec![None, Some(sid(1010))]);
    assert_eq!(sweep(&mut fake), SweepExit::TasksLeft);
    assert_eq!(fake.cancelled(), [1]);
}

#[test]
fn the_refusals_touch_nothing_and_no_pool_is_nothing_to_sweep() {
    let refused = |fake: &mut Fake, exit: SweepExit, case: &str| {
        fake.task("\\", "pool", user("S-1-5-21-1-2-3-1010"));
        fake.jobs = Ok(vec![Some(sid(1010))]);
        assert_eq!(sweep(fake), exit, "{case}");
        assert!(fake.done.is_empty(), "{case}: {:?}", fake.done);
        assert!(fake.logged[0].0, "{case}");
    };
    let mut fake = Fake::new(&[1010]);
    fake.system = false;
    refused(&mut fake, SweepExit::NotAllowed, "not SYSTEM");
    let mut fake = Fake::new(&[1010]);
    fake.exe = PathBuf::from(r"C:\Users\lawyer\AppData\Local\Clauseo\clauseo-sandbox-setup.exe");
    refused(&mut fake, SweepExit::NotAllowed, "not the admin copy");
    let mut fake = Fake::new(&[1010]);
    fake.pool = Err("PoolSids is malformed".to_owned());
    refused(&mut fake, SweepExit::NoRecord, "no record");
    let mut fake = Fake::new(&[1010, 500]);
    fake.not_pool = true;
    refused(&mut fake, SweepExit::NotPool, "not a pool user");
    for pool in [None, Some(Vec::new())] {
        let mut fake = Fake::new(&[]);
        fake.pool = Ok(pool.map(|users| Pool {
            owner: Sid::parse(OWNER).unwrap(),
            users,
        }));
        fake.task("\\", "any", user("S-1-5-21-1-2-3-1010"));
        fake.jobs = Ok(vec![Some(sid(1010))]);
        assert_eq!(sweep(&mut fake), SweepExit::Done);
        assert!(fake.done.is_empty(), "{:?}", fake.done);
        assert!(!fake.logged[0].0);
    }
}

#[test]
fn the_exit_codes_follow_the_stop_codes_and_are_never_shared() {
    use crate::windows::setup::stop::StopExit;
    let all = [
        SweepExit::Done,
        SweepExit::NotAllowed,
        SweepExit::NoRecord,
        SweepExit::NotPool,
        SweepExit::TasksLeft,
        SweepExit::JobsLeft,
        SweepExit::NotReserved,
        SweepExit::NotRecorded,
    ];
    let codes: Vec<u8> = all.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(codes, [0, 53, 54, 55, 56, 57, 58, 59]);
    let stop = StopExit::NotReserved.as_code();
    assert_eq!(stop, 50, "the stop's last code");
    let mut by_code = BTreeMap::new();
    for exit in all {
        assert!(by_code.insert(exit.as_code(), exit).is_none());
    }
    assert!(codes[1..].iter().all(|code| *code > stop));
    // 51 and 52 are the WSL deny task's (`WslDenyExit`: not allowed, and the upgrade failed with
    // the deny applied).
    assert!(!codes.contains(&51) && !codes.contains(&52));
}
