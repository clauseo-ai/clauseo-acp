//! Tests of what the leader reads, its hidden subcommand, its replacement when it dies, and the
//! error when xtask cannot find itself. `xtask/tests/process.rs` tests that it stops a command's
//! tree when xtask dies, a replaced leader included.

use super::*;
use crate::process::{Cmd, Program, TEST_TIMEOUT, run_quiet};
use crate::{Cli, Commands};
use clap::{CommandFactory, Parser};
use std::path::Path;

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

#[test]
fn a_dead_leader_is_replaced_when_the_next_command_starts() {
    let cmd = Cmd::new(Program::Named("true"), Vec::<String>::new(), TEST_TIMEOUT);
    run_quiet(&cmd, Path::new(".")).expect("true runs, and the leader starts");
    kill_for_test();
    let ran = run_quiet(&cmd, Path::new(".")).expect("a new leader takes the command");
    assert!(ran.success);
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
