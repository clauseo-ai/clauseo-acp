//! Tests of what the leader reads, and of its hidden subcommand. `xtask/tests/process.rs` tests
//! that it stops a command's tree when xtask dies.

use super::*;
use crate::{Cli, Commands};
use clap::{CommandFactory, Parser};

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
