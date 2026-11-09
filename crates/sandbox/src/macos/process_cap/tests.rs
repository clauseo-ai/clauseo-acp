use super::*;

fn proc(pid: u32, parent: u32, group: u32, start: u64) -> Proc {
    Proc {
        pid,
        parent,
        group,
        start,
    }
}

fn pids(found: &[Proc]) -> Vec<u32> {
    let mut pids: Vec<u32> = found.iter().map(|p| p.pid).collect();
    pids.sort_unstable();
    pids
}

#[test]
fn the_limit_keeps_the_reserve_below_the_per_user_count() {
    // This Mac: kern.maxprocperuid 4000, hard limit 6000 (kern.maxproc).
    assert_eq!(process_limit(4_000, 6_000).unwrap(), 3_488);
    assert_eq!(process_limit(4_000, u64::MAX).unwrap(), 3_488);
}

#[test]
fn the_limit_is_never_above_the_hard_limit() {
    assert_eq!(process_limit(4_000, 2_000).unwrap(), 2_000);
    assert_eq!(process_limit(4_000, 3_488).unwrap(), 3_488);
}

#[test]
fn a_per_user_count_under_the_floor_is_refused() {
    let refused = process_limit(MIN_PER_USER - 1, 6_000).unwrap_err();
    assert!(
        matches!(
            refused,
            clauseo_process::Error::ProcessLimitTooLow {
                per_user: 1_023,
                needed: 1_024
            }
        ),
        "{refused:?}"
    );
    assert!(
        refused
            .to_string()
            .starts_with("This Mac's process limit is too low for Clauseo's Safe workspace"),
        "{refused}"
    );
    assert!(process_limit(0, 6_000).is_err());
}

#[test]
fn the_floor_itself_is_accepted_with_the_reserve_taken() {
    assert_eq!(process_limit(MIN_PER_USER, 6_000).unwrap(), 512);
}

#[test]
fn descendants_are_found_by_parent_and_by_group() {
    let all = [
        proc(100, 1, 100, 10),   // the agent
        proc(101, 100, 100, 11), // its child
        proc(102, 101, 102, 12), // a grandchild that called setsid
        proc(103, 102, 102, 13), // and its child
        proc(104, 1, 100, 14),   // an orphan still in the group
        proc(200, 1, 200, 5),    // unrelated
        proc(201, 200, 200, 6),  // unrelated child
    ];
    assert_eq!(pids(&descendants(100, 10, &all)), [101, 102, 103, 104]);
}

#[test]
fn a_reused_pid_is_not_a_descendant() {
    let all = [
        proc(100, 1, 100, 10),
        // Its parent pid is the agent's, but it started first: a pid reused, or a stale read.
        proc(101, 100, 101, 9),
        proc(102, 101, 101, 20),
        // A group member older than the agent is not the agent's.
        proc(103, 1, 100, 3),
    ];
    assert_eq!(descendants(100, 10, &all), []);
}

#[test]
fn an_orphan_that_left_the_group_is_not_found() {
    // The residual (0038): setsid, then reparented to launchd.
    let all = [proc(100, 1, 100, 10), proc(105, 1, 105, 12)];
    assert_eq!(descendants(100, 10, &all), []);
}

/// A tree of `children` direct children of agent 100, each with `each` children of its own.
fn tree(children: u32, each: u32) -> Vec<Proc> {
    let mut found = Vec::new();
    for child in 0..children {
        let pid = 1_000 + child;
        found.push(proc(pid, 100, 100, 20));
        for grandchild in 0..each {
            found.push(proc(10_000 + child * 1_000 + grandchild, pid, 100, 30));
        }
    }
    found
}

#[test]
fn a_bomb_in_a_command_spares_the_agent() {
    // One command (the agent's only child) whose tree grew past the cap.
    let found = tree(1, 400);
    assert!(found.len() > AGENT_PROCESS_CAP);
    assert_eq!(verdict(100, &found), Verdict::SpareAgent);
    // A handful of commands, each with its own workers.
    assert_eq!(verdict(100, &tree(6, 60)), Verdict::SpareAgent);
}

#[test]
fn an_agent_that_forks_itself_is_killed() {
    // The agent starts processes in a loop: its own children are the tree.
    assert_eq!(verdict(100, &tree(300, 0)), Verdict::KillAgent);
    // Most of the tree is enough: 200 of its own beside one command's 100 workers.
    let mut mixed = tree(200, 0);
    mixed.extend((0..100).map(|n| proc(50_000 + n, 1_000, 100, 30)));
    assert_eq!(verdict(100, &mixed), Verdict::KillAgent);
    // Each child with a child of its own: exactly half, so the agent stays.
    assert_eq!(verdict(100, &tree(200, 1)), Verdict::SpareAgent);
}

#[test]
fn half_and_half_spares_the_agent() {
    let mut found = tree(150, 0);
    found.extend((0..150).map(|n| proc(50_000 + n, 1_000, 100, 30)));
    assert_eq!(verdict(100, &found), Verdict::SpareAgent);
}

#[test]
fn the_notice_is_windows_text() {
    assert_eq!(
        TOO_MANY_PROGRAMS,
        "A task tried to start too many programs at once, so Clauseo stopped it. Ask the \
         assistant to try a smaller step."
    );
}
