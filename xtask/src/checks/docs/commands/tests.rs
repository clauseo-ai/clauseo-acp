//! Tests of how `cargo xtask` commands parse, and which text holds them.

use super::*;

/// The first line of clap's error for `command` (`cargo xtask` is its start), `None` when it
/// parses.
fn error_for(command: &str) -> Option<String> {
    let mut argv = vec!["cargo xtask"];
    argv.extend(command.split_whitespace().skip(2));
    parse(&argv).err()
}

/// The commands `commands_in` finds in `text`.
fn commands_of(text: &str) -> Vec<String> {
    commands_in(text)
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
fn help_parses_at_every_level_and_bare_help_is_no_command() {
    assert_eq!(error_for("cargo xtask --help"), None);
    assert_eq!(error_for("cargo xtask check --help"), None);
    assert_eq!(error_for("cargo xtask buildtime -h"), None);
}

#[test]
fn help_does_not_hide_a_bad_command_after_it() {
    assert_eq!(
        error_for("cargo xtask --help frobnicate").as_deref(),
        Some("error: unrecognized subcommand 'frobnicate'")
    );
    assert_eq!(
        error_for("cargo xtask check --help --frobnicate").as_deref(),
        Some("error: unexpected argument '--frobnicate' found")
    );
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
fn a_command_is_found_after_a_prompt_assignments_and_separators() {
    assert_eq!(commands_of("$ cargo xtask check"), ["cargo xtask check"]);
    assert_eq!(
        commands_of("RUST_LOG=1 CI=1 cargo xtask check --quick"),
        ["cargo xtask check --quick"]
    );
    assert_eq!(
        commands_of("cargo xtask check && cargo xtask frobnicate"),
        ["cargo xtask check", "cargo xtask frobnicate"]
    );
    assert_eq!(
        commands_of("cd ui; cargo xtask setup"),
        ["cargo xtask setup"]
    );
    assert_eq!(
        commands_of("cargo xtask setup | tee x"),
        ["cargo xtask setup"]
    );
}

#[test]
fn a_command_inside_other_words_or_after_a_comment_is_not_one_of_these() {
    assert_eq!(commands_of("the cargo xtask command"), Vec::<String>::new());
    assert_eq!(commands_of("cargo xtasks check"), Vec::<String>::new());
    assert_eq!(commands_of("# cargo xtask check"), Vec::<String>::new());
    assert_eq!(
        commands_of("cargo xtask check  # changed crates only"),
        ["cargo xtask check"]
    );
}

#[test]
fn an_assignment_is_a_name_then_equals() {
    assert!(is_assignment("A_1=x"));
    assert!(is_assignment("A="));
    assert!(!is_assignment("1A=x"));
    assert!(!is_assignment("--x=1"));
    assert!(!is_assignment("cargo"));
}

#[test]
fn a_backslash_continues_a_code_line_onto_the_next() {
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
        logical_lines(&lines),
        [
            (7, "cargo xtask check --frobnicate".to_owned()),
            (9, "echo done".to_owned()),
        ]
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

#[test]
fn a_bare_command_or_help_is_read_without_clap_s_help_text() {
    assert_eq!(error_for("cargo xtask"), Some(String::new()));
    assert_eq!(error_for("cargo xtask -h"), None);
    let found = problem("a.md", 1, "cargo xtask").expect("it does not parse");
    assert_eq!(
        found.message,
        "the xtask command `cargo xtask` does not parse"
    );
}

#[test]
fn shell_redirections_and_a_closing_paren_are_not_arguments() {
    assert_eq!(commands_of("cargo xtask check 2>&1"), ["cargo xtask check"]);
    assert_eq!(
        commands_of("cargo xtask check > out.txt"),
        ["cargo xtask check"]
    );
    assert_eq!(
        commands_of("cargo xtask check >>out.txt"),
        ["cargo xtask check"]
    );
    assert_eq!(
        commands_of("cargo xtask check &> out"),
        ["cargo xtask check"]
    );
    assert_eq!(commands_of("cargo xtask check < in"), ["cargo xtask check"]);
    assert_eq!(commands_of("(cargo xtask check)"), ["cargo xtask check"]);
}

#[test]
fn a_subshell_substitution_or_prefix_word_before_the_command_is_skipped() {
    assert_eq!(
        commands_of("(cd x && cargo xtask frobnicate)"),
        ["cargo xtask frobnicate"]
    );
    assert_eq!(commands_of("$(cargo xtask check)"), ["cargo xtask check"]);
    assert_eq!(
        commands_of("time env cargo xtask check"),
        ["cargo xtask check"]
    );
    assert_eq!(commands_of("exec cargo xtask check"), ["cargo xtask check"]);
    assert_eq!(commands_of("echo cargo xtask check"), Vec::<String>::new());
}
