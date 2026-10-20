//! Tests of what the leader reads, how it stops groups, its hidden subcommand, its replacement
//! when it dies, the groups xtask keeps for it, and the error when xtask cannot find itself.
//! `xtask/tests/process.rs` tests that it stops a command's tree when xtask dies, a replaced
//! leader included.

use super::*;
use crate::clock::timed;
use crate::process::running::Limits;
use crate::process::tests::tree::{Grandchild, SHORT_WAIT, SURVIVOR, WRAPPER, start};
use crate::process::{Cmd, Program, TEST_TIMEOUT, run_quiet};
use crate::{Cli, Commands};
use clap::{CommandFactory, Parser};
use std::path::Path;
use std::thread;

#[test]
fn the_leader_stops_the_groups_registered_and_not_released() {
    let input = "+12\n+13\n-12\n+14\n";
    assert_eq!(registered(input.as_bytes()), BTreeSet::from([13, 14]));
}

#[test]
fn the_leader_skips_lines_xtask_does_not_write() {
    let input = "+0\n+-5\n12\n+x\n\n+7\n";
    assert_eq!(registered(input.as_bytes()), BTreeSet::from([7]));
}

#[test]
fn the_leader_subcommand_parses_and_is_hidden_from_help() {
    let cli = Cli::try_parse_from(["xtask", LEADER_SUBCOMMAND]).expect("it parses");
    assert!(matches!(cli.command, Commands::ProcessLeader));
    let test = Cli::try_parse_from(["xtask", "process-test", "capture", "true"]).expect("parses");
    assert!(matches!(test.command, Commands::ProcessTest { .. }));
    let help = Cli::command().render_long_help().to_string();
    assert!(!help.contains("process-"), "{help}");
    assert!(help.contains("buildtime"), "{help}");
}

/// The pid of the leader now running.
fn leader_pid() -> u32 {
    current().expect("a command started the leader").0
}

#[test]
fn a_dead_leader_is_replaced_when_the_next_command_starts() {
    let cmd = Cmd::new(Program::Named("true"), Vec::<String>::new(), TEST_TIMEOUT);
    run_quiet(&cmd, Path::new(".")).expect("true runs, and the leader starts");
    let killed = leader_pid();
    kill_for_test();
    let ran = run_quiet(&cmd, Path::new(".")).expect("a new leader takes the command");
    assert!(ran.success);
    assert_ne!(leader_pid(), killed, "the killed leader was replaced");
}

#[test]
fn a_group_is_registered_while_its_run_lasts_and_released_after() {
    let (running, reader) = start(WRAPPER, Limits::of(TEST_TIMEOUT));
    let tree = Grandchild::of(reader);
    let group = Pid::from_raw(tree.group());
    let during = current().expect("a leader runs").1.contains(&group);
    drop(running);
    assert_eq!(tree.gone(), b"");
    assert!(during, "group {group} was registered while it ran");
    let after = current().expect("a leader runs").1;
    assert!(!after.contains(&group), "group {group} was released");
}

#[test]
fn the_leader_kills_a_group_that_ignores_sigterm_once_its_grace_is_over() {
    let (running, reader) = start(
        &format!("trap '' TERM; {WRAPPER}"),
        Limits::of(TEST_TIMEOUT),
    );
    let tree = Grandchild::of(reader);
    let ((), secs) = timed(|| stop(&BTreeSet::from([tree.group()]), SHORT_WAIT));
    if secs < SHORT_WAIT.as_secs_f64() {
        tree.fail(&format!(
            "the leader sent SIGKILL after {secs} s, before its grace"
        ));
    }
    assert_eq!(tree.gone(), b"");
    drop(running);
}

#[test]
fn the_leader_stops_waiting_once_its_groups_are_empty() {
    let (running, reader) = start(WRAPPER, Limits::of(TEST_TIMEOUT));
    let tree = Grandchild::of(reader);
    let groups = BTreeSet::from([tree.group()]);
    let (sender, stopped) = mpsc::channel();
    // A grace far past `SURVIVOR`, so only a leader that sees the empty group returns in time.
    thread::spawn(move || {
        stop(&groups, TEST_TIMEOUT);
        drop(sender.send(groups));
    });
    if stopped.recv_timeout(SURVIVOR).is_err() {
        tree.fail(&format!(
            "the leader still waited {SURVIVOR:?} after SIGTERM ended it"
        ));
    }
    assert_eq!(tree.gone(), b"");
    drop(running);
}

#[test]
fn xtask_not_finding_its_own_binary_is_not_a_missing_program() {
    let missing = io::Error::new(io::ErrorKind::NotFound, "no /proc");
    let error = own_binary(Err(missing)).expect_err("current_exe failed");
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(
        error.to_string(),
        "could not find xtask's own binary, which runs its process leader: no /proc"
    );
}
