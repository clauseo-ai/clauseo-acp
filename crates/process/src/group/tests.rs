//! The stamps and the group listing against real processes this test starts (Linux and macOS).
//! Every process signalled here is one of the test's own children.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#![expect(
    clippy::disallowed_methods,
    reason = "the test starts its own sleepers with std, and waits out the start clock's unit"
)]

use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::time::Duration;

use super::{
    Member, Presence, StartStamp, boot_stamp, group_members, kill_group, kill_member, presence,
    session_id, start_stamp,
};

/// `sleep 600` in the process group `group` (0: a new group it leads).
fn sleeper(group: i32) -> Child {
    Command::new("sleep")
        .arg("600")
        .process_group(group)
        .spawn()
        .expect("sleep starts")
}

/// Kills and reaps the test's own `child`.
fn end(mut child: Child) {
    drop(child.kill());
    drop(child.wait());
}

/// The pids of group `pgid`'s running members, sorted.
fn pids(pgid: u32) -> Vec<u32> {
    let mut pids: Vec<u32> = group_members(pgid)
        .expect("the group listing")
        .iter()
        .map(|member| member.pid)
        .collect();
    pids.sort_unstable();
    pids
}

#[test]
fn a_start_stamp_is_stable_and_a_later_child_has_a_later_one() {
    let me = std::process::id();
    let first = start_stamp(me).expect("this process's start");
    assert_eq!(start_stamp(me), Some(first), "the same on a second read");
    assert_eq!(StartStamp::decode(&first.encode()), Some(first));
    assert_eq!(StartStamp::decode("12x"), None);
    assert_eq!(StartStamp::decode(""), None);
    // Past the start clock's unit (10 ms on Linux), so the child's start is a later tick.
    std::thread::sleep(Duration::from_millis(30));
    let child = sleeper(0);
    let theirs = start_stamp(child.id()).expect("the child's start");
    end(child);
    assert!(theirs > first, "{theirs} after {first}");
}

#[test]
fn the_boot_stamp_is_stable() {
    let boot = boot_stamp().expect("the boot stamp");
    assert_ne!(boot, "");
    assert_eq!(boot_stamp(), Some(boot));
}

#[test]
fn the_member_listing_finds_a_groups_members_and_no_others() {
    let leader = sleeper(0);
    let pgid = leader.id();
    let member = sleeper(i32::try_from(pgid).unwrap());
    let other = sleeper(0);
    let mut want = vec![pgid, member.id()];
    want.sort_unstable();
    let found = pids(pgid);
    let other_pid = other.id();
    let starts = group_members(pgid).unwrap();
    end(other);
    assert_eq!(
        found, want,
        "the leader and its member, not the other sleeper"
    );
    assert!(!found.contains(&std::process::id()));
    assert!(!found.contains(&other_pid));
    assert!(
        starts.iter().all(|m| start_stamp(m.pid) == Some(m.start)),
        "each member carries its start"
    );
    // The group's kill ends both, and the listing then holds none running.
    assert!(kill_group(pgid));
    end(leader);
    end(member);
    assert_eq!(pids(pgid), Vec::<u32>::new());
}

#[test]
fn kills_refuse_group_zero_one_this_group_and_a_wrong_start() {
    assert!(!kill_group(0));
    assert!(!kill_group(1));
    let own = nix::unistd::getpgrp().as_raw().unsigned_abs();
    assert!(!kill_group(own), "never this process's own group");
    let child = sleeper(0);
    let start = start_stamp(child.id()).unwrap();
    let wrong = Member {
        pid: child.id(),
        session: 0,
        start: StartStamp(start.0 + 1),
    };
    assert!(
        !kill_member(&wrong),
        "a start that differs is another process"
    );
    assert_eq!(start_stamp(child.id()), Some(start), "still running");
    let right = Member {
        pid: child.id(),
        session: 0,
        start,
    };
    assert!(kill_member(&right));
    end(child);
}

#[test]
fn members_carry_their_session_and_presence_tells_a_zombie_from_a_gone_pid() {
    let me = std::process::id();
    let ours = session_id(me).expect("this process's session");
    let mut child = sleeper(0);
    let pid = child.id();
    let start = start_stamp(pid).unwrap();
    assert_eq!(
        session_id(pid),
        Some(ours),
        "process_group(0) keeps the session"
    );
    assert!(
        group_members(pid)
            .unwrap()
            .iter()
            .all(|member| member.session == ours)
    );
    assert_eq!(presence(pid), Presence::There(Some(start)));
    // Killed but not reaped: a zombie still holds its pid, and on Linux its start reads.
    child.kill().unwrap();
    while start_stamp(pid).is_some() {
        std::thread::sleep(Duration::from_millis(5));
    }
    #[cfg(target_os = "linux")]
    assert_eq!(
        presence(pid),
        Presence::There(Some(start)),
        "a zombie is there"
    );
    #[cfg(target_os = "macos")]
    assert!(
        matches!(presence(pid), Presence::There(_)),
        "a zombie is there"
    );
    child.wait().unwrap();
    assert_eq!(presence(pid), Presence::Gone, "reaped, the pid is free");
}
