//! Tests of how `cargo xtask` commands parse, and which lines are commands.

use super::*;

/// The first line of clap's error for `command` (`cargo xtask` is added), `None` when it parses.
fn error_for(command: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().skip(2).collect();
    let mut argv = vec!["cargo xtask"];
    argv.extend(words);
    parse(&argv).err()
}

#[test]
fn the_real_commands_parse() {
    for command in [
        "cargo xtask check --quick",
        "cargo xtask check",
        "cargo xtask setup --binaries",
        "cargo xtask buildtime rust --runs 3 --json",
        "cargo xtask buildtime web",
    ] {
        assert_eq!(error_for(command), None, "{command}");
    }
}

#[test]
fn help_parses_at_every_level() {
    assert_eq!(error_for("cargo xtask --help"), None);
    assert_eq!(error_for("cargo xtask check --help"), None);
}

#[test]
fn an_unknown_subcommand_and_an_unknown_flag_do_not_parse() {
    assert_eq!(
        error_for("cargo xtask frobnicate").as_deref(),
        Some("error: unrecognized subcommand 'frobnicate'")
    );
    assert_eq!(
        error_for("cargo xtask check --frobnicate").as_deref(),
        Some("error: unexpected argument '--frobnicate' found")
    );
}

#[test]
fn a_value_outside_its_range_does_not_parse() {
    assert!(error_for("cargo xtask buildtime --runs 0").is_some());
}

#[test]
fn a_span_is_a_command_only_when_it_starts_with_the_two_words() {
    assert!(starts_with_xtask("cargo xtask check"));
    assert!(starts_with_xtask("  cargo   xtask"));
    assert!(!starts_with_xtask("cargo xtasks check"));
    assert!(!starts_with_xtask("run cargo xtask"));
}

#[test]
fn a_fenced_line_with_a_shell_comment_is_checked_without_it() {
    let lines = [Line {
        line: 4,
        text: "  cargo xtask check --quick  # changed crates only".to_owned(),
    }];
    assert_eq!(
        fenced_commands(&lines),
        [(
            4,
            "cargo xtask check --quick  # changed crates only".to_owned()
        )]
    );
    assert_eq!(problem("a.md", 4, &fenced_commands(&lines)[0].1), None);
}

#[test]
fn a_backslash_continues_a_fenced_command_onto_the_next_line() {
    let lines = [
        Line {
            line: 7,
            text: "cargo xtask check \\".to_owned(),
        },
        Line {
            line: 8,
            text: "  --frobnicate".to_owned(),
        },
        Line {
            line: 9,
            text: "echo done".to_owned(),
        },
    ];
    assert_eq!(
        fenced_commands(&lines),
        [(7, "cargo xtask check --frobnicate".to_owned())]
    );
}

#[test]
fn a_violation_names_the_command_and_gives_clap_s_first_line_when_it_is_short() {
    let found = problem("a.md", 3, "cargo xtask frobnicate").expect("it does not parse");
    assert_eq!(
        found.message,
        "the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'"
    );
    assert_eq!(found.line, 3);
}
