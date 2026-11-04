use std::path::PathBuf;

use super::{Cutoff, Proc, below, held, new_since};

/// A cutoff past every start these tests use.
const LATE: Cutoff = Cutoff(u64::MAX);

fn proc(pid: u32, parent: Option<u32>, start: u64, exe: &str) -> Proc {
    Proc {
        pid,
        parent,
        start,
        leads_group: false,
        exe: Some(PathBuf::from(exe)),
    }
}

fn pids(processes: &[Proc]) -> Vec<u32> {
    processes.iter().map(|p| p.pid).collect()
}

#[test]
fn only_what_the_earlier_listing_lacks_is_new() {
    let old = [proc(10, Some(1), 100, "/bin/app-server")];
    let now = [
        proc(10, Some(1), 100, "/bin/app-server"),
        proc(20, Some(10), 200, "/bin/zsh"),
        proc(21, Some(20), 210, "/bin/sleep"),
    ];
    assert_eq!(pids(&new_since(&old, &now, LATE, &[])), [20, 21]);
}

#[test]
fn a_reused_pid_is_a_new_process() {
    let old = [proc(10, Some(1), 100, "/bin/app-server")];
    let now = [proc(10, Some(1), 999, "/bin/other")];
    assert_eq!(pids(&new_since(&old, &now, LATE, &[])), [10]);
}

#[test]
fn a_process_started_at_or_after_the_cutoff_is_never_a_target() {
    let now = [
        proc(20, Some(1), 499, "/bin/sleep"),
        proc(30, Some(1), 500, "/bin/next-turn"),
        proc(31, Some(1), 501, "/bin/next-turn"),
    ];
    assert_eq!(pids(&new_since(&[], &now, Cutoff(500), &[])), [20]);
}

#[test]
fn a_kept_executable_and_everything_under_it_stay() {
    let now = [
        proc(30, Some(1), 300, "/usr/bin/clauseo-mcp"),
        proc(31, Some(30), 310, "/bin/helper"),
        proc(32, Some(31), 320, "/bin/helper-child"),
        proc(40, Some(1), 400, "/bin/sleep"),
    ];
    let keep = [PathBuf::from("/usr/bin/clauseo-mcp")];
    assert_eq!(pids(&new_since(&[], &now, LATE, &keep)), [40]);
}

#[test]
fn a_process_with_no_known_executable_is_never_kept() {
    let mut unnamed = proc(50, Some(1), 500, "");
    unnamed.exe = None;
    let keep = [PathBuf::from("")];
    assert_eq!(pids(&new_since(&[], &[unnamed], LATE, &keep)), [50]);
}

#[test]
fn nothing_new_is_nothing() {
    let old = [proc(10, Some(1), 100, "/bin/a")];
    assert_eq!(pids(&new_since(&old, &old, LATE, &[])), [0_u32; 0]);
}

#[test]
fn the_second_pass_takes_the_targets_and_what_they_started_only() {
    let targets = vec![proc(20, Some(10), 200, "/bin/zsh")];
    let again = [
        proc(20, Some(10), 200, "/bin/zsh"),
        // Started by the target during the grace: after any cutoff, still taken.
        proc(21, Some(20), 900, "/bin/sleep"),
        proc(22, Some(21), 950, "/bin/sleep-child"),
        // The next turn's command, a sibling: never taken.
        proc(30, Some(10), 910, "/bin/next-turn"),
    ];
    assert_eq!(pids(&below(targets, &again, &[])), [20, 21, 22]);
}

#[test]
fn a_dead_targets_child_is_taken_but_a_reused_pids_child_is_not() {
    // The target, ended by the first pass, is gone; its child keeps its pid as parent
    // (Windows keeps a dead parent's pid).
    let targets = vec![proc(20, Some(10), 200, "/bin/cmd")];
    let orphan = [proc(21, Some(20), 300, "/bin/child")];
    assert_eq!(pids(&below(targets.clone(), &orphan, &[])), [20, 21]);
    // Pid 20 now names another process, which started a child of its own.
    let reused = [
        proc(20, Some(10), 800, "/bin/other"),
        proc(23, Some(20), 810, "/bin/others-child"),
    ];
    assert_eq!(pids(&below(targets, &reused, &[])), [20]);
}

#[test]
fn a_child_older_than_its_parent_is_not_its_child() {
    let targets = vec![proc(20, Some(10), 200, "/bin/cmd")];
    let again = [proc(21, Some(20), 150, "/bin/older")];
    assert_eq!(pids(&below(targets, &again, &[])), [20]);
}

#[test]
fn the_agents_conhost_is_never_a_target_and_a_new_commands_goes_with_it() {
    // Agent 1's own conhost started with it, before the snapshot; so did the first command.
    let conhost = r"C:\Windows\System32\conhost.exe";
    let old = [
        proc(5, Some(1), 50, conhost),
        proc(10, Some(1), 100, "fake_agent.exe"),
    ];
    // The turn started a console command with no console, which got a conhost of its own.
    let now = [
        proc(5, Some(1), 50, conhost),
        proc(10, Some(1), 100, "fake_agent.exe"),
        proc(20, Some(1), 200, "cmd.exe"),
        proc(21, Some(20), 210, conhost),
    ];
    let targets = new_since(&old, &now, Cutoff(500), &[]);
    assert_eq!(pids(&targets), [20, 21]);
    // The second pass takes from what the targets started only, never the agent's conhost.
    assert_eq!(pids(&below(targets, &now, &[])), [20, 21]);
}

#[test]
fn a_target_with_no_hold_is_no_parent_in_the_second_pass() {
    // Target 20 had ended before the first pass held it, so its pid could go to a later process
    // that started 21 and ended: 21 names 20 as parent but must not be ended.
    let targets = vec![
        proc(20, Some(10), 200, "cmd.exe"),
        proc(30, Some(10), 300, "cmd.exe"),
    ];
    let (held, holds) = held(targets, |process| (process.pid == 30).then_some(()));
    assert_eq!((pids(&held), holds.len()), (vec![30], 1));
    let again = [
        proc(21, Some(20), 900, "next-turn.exe"),
        proc(31, Some(30), 910, "child.exe"),
    ];
    assert_eq!(pids(&below(held, &again, &[])), [30, 31]);
}
