//! Tests of how `cargo xtask` commands parse, and which text holds them.

use super::*;

/// The first line of clap's error for `command` (`cargo xtask` is its start), `None` when it
/// parses.
fn error_for(command: &str) -> Option<String> {
    let mut argv = vec!["cargo xtask"];
    argv.extend(command.split_whitespace().skip(2));
    parse(&argv, false).err()
}

/// The commands `commands_in` finds in `line`, as their words joined by spaces.
fn commands_of(line: &str) -> Vec<String> {
    commands_in(line)
        .into_iter()
        .map(|command| command.words.join(" "))
        .collect()
}

/// `text` as a command with no placeholder after it.
fn command(text: &str) -> Command {
    Command {
        words: text.split_whitespace().map(str::to_owned).collect(),
        placeholder: false,
    }
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
fn a_command_is_found_anywhere_in_the_words_but_not_in_other_words_or_a_comment() {
    assert_eq!(
        commands_of("the cargo xtask command"),
        ["cargo xtask command"]
    );
    assert_eq!(commands_of("cargo xtasks check"), Vec::<String>::new());
    assert_eq!(commands_of("mycargo xtask check"), Vec::<String>::new());
    assert_eq!(commands_of("# cargo xtask check"), Vec::<String>::new());
    assert_eq!(
        commands_of("cargo xtask check  # changed crates only"),
        ["cargo xtask check"]
    );
}

#[test]
fn a_violation_names_the_command_and_gives_clap_s_first_line_when_it_is_short() {
    let found = problem("a.md", 3, &command("cargo xtask frobnicate")).expect("it does not parse");
    assert_eq!(
        found.message,
        "the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'"
    );
    assert_eq!(found.line, 3);
}

#[test]
fn a_bare_command_names_no_subcommand_and_bare_help_parses() {
    assert_eq!(
        error_for("cargo xtask").as_deref(),
        Some("it names no subcommand")
    );
    assert_eq!(error_for("cargo xtask -h"), None);
    let found = problem("a.md", 1, &command("cargo xtask")).expect("it does not parse");
    assert_eq!(
        found.message,
        "the xtask command `cargo xtask` does not parse: it names no subcommand"
    );
}

#[test]
fn a_placeholder_word_is_a_name_in_angle_brackets_or_an_ellipsis() {
    for word in ["<command>", "<n>", "...", "<path>..."] {
        assert!(is_placeholder(word), "{word} is a placeholder");
    }
    for word in ["<", ">", "<>", "<in", "out>", "2>&1", "check", "<a<b>"] {
        assert!(!is_placeholder(word), "{word} is not a placeholder");
    }
}

#[test]
fn a_placeholder_for_the_subcommand_gives_no_command_and_a_later_one_cuts_the_words() {
    assert_eq!(
        commands_of("cargo xtask <command> --help"),
        Vec::<String>::new()
    );
    assert_eq!(commands_of("cargo xtask ..."), Vec::<String>::new());
    assert_eq!(
        commands_in("cargo xtask buildtime --runs <n> --json"),
        [Command {
            words: ["cargo", "xtask", "buildtime", "--runs"]
                .map(str::to_owned)
                .to_vec(),
            placeholder: true,
        }]
    );
}

#[test]
fn before_a_placeholder_a_missing_word_parses_and_a_wrong_one_does_not() {
    let cut = |text: &str| {
        let mut argv = vec!["cargo xtask"];
        argv.extend(text.split_whitespace().skip(2));
        parse(&argv, true).err()
    };
    assert_eq!(cut("cargo xtask buildtime --runs"), None);
    assert_eq!(
        cut("cargo xtask frobnicate").as_deref(),
        Some("error: unrecognized subcommand 'frobnicate'")
    );
    assert_eq!(
        cut("cargo xtask check --frobnicate").as_deref(),
        Some("error: unexpected argument '--frobnicate' found")
    );
    assert!(error_for("cargo xtask buildtime --runs").is_some());
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
fn whatever_comes_before_the_command_is_skipped() {
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
    assert_eq!(commands_of("echo cargo xtask check"), ["cargo xtask check"]);
    for text in [
        "sudo -E cargo xtask frobnicate",
        "sudo -u me cargo xtask frobnicate",
        "env -i cargo xtask frobnicate",
        "time -p cargo xtask frobnicate",
        "nohup cargo xtask frobnicate",
        "x=$(cargo xtask frobnicate)",
        "% cargo xtask frobnicate",
        "PS> cargo xtask frobnicate",
        "echo `cargo xtask frobnicate`",
    ] {
        assert_eq!(commands_of(text), ["cargo xtask frobnicate"], "{text}");
    }
}

#[test]
fn shell_punctuation_after_a_word_ends_the_command() {
    assert_eq!(commands_of("cargo xtask check &"), ["cargo xtask check"]);
    assert_eq!(commands_of("cargo xtask check&"), ["cargo xtask check"]);
    assert_eq!(commands_of("$(cargo xtask)"), ["cargo xtask"]);
    assert_eq!(
        commands_of("(cargo +nightly xtask check)"),
        ["cargo xtask check"]
    );
}

#[test]
fn a_toolchain_after_cargo_is_skipped() {
    assert_eq!(
        commands_of("cargo +nightly xtask check"),
        ["cargo xtask check"]
    );
    assert_eq!(commands_of("cargo + xtask check"), Vec::<String>::new());
}

#[test]
fn words_are_split_as_a_shell_splits_them() {
    assert_eq!(commands_of("cargo xtask \"check\""), ["cargo xtask check"]);
    assert_eq!(
        commands_of("FOO=\"a b\" cargo xtask frobnicate"),
        ["cargo xtask frobnicate"]
    );
    assert_eq!(
        commands_of("sudo cargo xtask frobnicate"),
        ["cargo xtask frobnicate"]
    );
    assert_eq!(
        commands_of("echo 'a && b; c | d' && cargo xtask check"),
        ["cargo xtask check"]
    );
    assert_eq!(
        commands_of("cargo xtask check # it is fine; really"),
        ["cargo xtask check"]
    );
}

#[test]
fn every_command_on_a_line_is_checked_and_a_comment_ends_it() {
    assert_eq!(
        commands_of("cargo xtask check && cargo xtask setup; cargo xtask frobnicate"),
        [
            "cargo xtask check",
            "cargo xtask setup",
            "cargo xtask frobnicate"
        ]
    );
    assert_eq!(
        commands_of("cargo xtask check # then cargo xtask frobnicate"),
        ["cargo xtask check"]
    );
}

#[test]
fn a_quoted_hash_is_a_word_and_not_a_comment() {
    assert_eq!(
        commands_of("x \"#a\" cargo xtask frobnicate"),
        ["cargo xtask frobnicate"]
    );
}

#[test]
fn cargo_flags_and_a_toolchain_before_xtask_are_skipped() {
    assert_eq!(
        commands_of("cargo --locked xtask frobnicate"),
        ["cargo xtask frobnicate"]
    );
    assert_eq!(
        commands_of("cargo -q +nightly xtask frobnicate"),
        ["cargo xtask frobnicate"]
    );
    assert_eq!(
        commands_of("cargo --color always xtask x"),
        Vec::<String>::new()
    );
}

#[test]
fn a_line_shlex_cannot_split_is_not_read() {
    assert_eq!(
        commands_of("cargo xtask check --name \"open"),
        Vec::<String>::new()
    );
    assert_eq!(
        commands_of("Don't run cargo xtask check"),
        Vec::<String>::new()
    );
}

#[test]
fn a_trailing_backslash_is_dropped_before_the_split() {
    assert_eq!(
        commands_of("cargo xtask frobnicate \\"),
        ["cargo xtask frobnicate"]
    );
    assert_eq!(commands_of("cargo xtask check\\  "), ["cargo xtask check"]);
}

#[test]
fn a_glued_second_command_is_not_read() {
    assert_eq!(commands_of("a;cargo xtask x"), Vec::<String>::new());
}

#[test]
fn a_block_with_no_info_string_or_a_shell_one_reads_every_line() {
    assert_eq!(
        read_line("", "Run cargo xtask check, then sudo cargo xtask setup"),
        Some("Run cargo xtask check, then sudo cargo xtask setup")
    );
    assert_eq!(
        read_line("Bash", "sudo cargo xtask check"),
        Some("sudo cargo xtask check")
    );
    assert_eq!(read_line("rust", "cargo xtask check"), None);
    assert_eq!(
        read_line("md", "Run cargo xtask to see the commands."),
        None
    );
}

#[test]
fn a_session_block_reads_only_the_prompted_lines() {
    assert_eq!(
        read_line("console", "$ cargo xtask check"),
        Some("cargo xtask check")
    );
    assert_eq!(
        read_line("Terminal", "  % cargo +nightly xtask check"),
        Some("cargo +nightly xtask check")
    );
    assert_eq!(
        read_line("console", "PS> cargo xtask check"),
        Some("cargo xtask check")
    );
    assert_eq!(
        read_line("console", "# then run cargo xtask check again"),
        None
    );
    assert_eq!(read_line("console", "> cargo xtask check passes now"), None);
    assert_eq!(read_line("console", "PS C:\\repo> cargo xtask check"), None);
    assert_eq!(
        read_line("console", "error: run cargo xtask setup first"),
        None
    );
    assert!(is_one_of("Console", &SESSIONS));
    assert!(is_one_of("PWSH", &SHELLS));
    assert!(!is_one_of("rust", &SHELLS));
}
