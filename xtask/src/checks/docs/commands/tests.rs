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

/// Asserts that the commands found in `line` are `expected`.
fn assert_reads(line: &str, expected: &[&str]) {
    assert_eq!(commands_of(line), expected, "{line}");
}

/// Asserts that `command` fails to parse with clap's first line `error`.
fn assert_error(command: &str, error: &str) {
    assert_eq!(error_for(command).as_deref(), Some(error), "{command}");
}

/// Asserts that `read_line` reads `line` in a block with info word `info` as `expected`.
fn assert_read(info: &str, line: &str, expected: Option<&str>) {
    assert_eq!(read_line(info, line), expected, "{info} {line}");
}

/// `text` as a command with no placeholder after it.
fn command(text: &str) -> Command {
    Command {
        words: text.split_whitespace().map(str::to_owned).collect(),
        placeholder: false,
    }
}

#[test]
fn the_real_commands_and_help_parse() {
    for command in [
        "cargo xtask check --quick",
        "cargo xtask check",
        "cargo xtask setup --binaries",
        "cargo xtask buildtime rust --runs 3 --json",
        "cargo xtask buildtime web",
        "cargo xtask --help",
        "cargo xtask check --help",
        "cargo xtask buildtime -h",
    ] {
        assert_eq!(error_for(command), None, "{command}");
    }
}

#[test]
fn an_unknown_subcommand_or_flag_does_not_parse_even_after_help() {
    let subcommand = "error: unrecognized subcommand 'frobnicate'";
    let flag = "error: unexpected argument '--frobnicate' found";
    for (command, error) in [
        ("cargo xtask frobnicate", subcommand),
        ("cargo xtask --help frobnicate", subcommand),
        ("cargo xtask check --frobnicate", flag),
        ("cargo xtask check --help --frobnicate", flag),
    ] {
        assert_error(command, error);
    }
}

#[test]
fn a_value_outside_its_range_does_not_parse() {
    assert!(error_for("cargo xtask buildtime --runs 0").is_some());
}

#[test]
fn a_command_is_found_after_a_prompt_assignments_and_separators() {
    assert_reads("$ cargo xtask check", &["cargo xtask check"]);
    assert_reads(
        "RUST_LOG=1 CI=1 cargo xtask check --quick",
        &["cargo xtask check --quick"],
    );
    assert_reads(
        "cargo xtask check && cargo xtask frobnicate",
        &["cargo xtask check", "cargo xtask frobnicate"],
    );
    assert_reads("cd ui; cargo xtask setup", &["cargo xtask setup"]);
    assert_reads("cargo xtask setup | tee x", &["cargo xtask setup"]);
}

#[test]
fn a_command_is_found_anywhere_in_the_words_but_not_in_other_words_or_a_comment() {
    assert_reads("the cargo xtask command", &["cargo xtask command"]);
    assert_reads("cargo xtasks check", &[]);
    assert_reads("mycargo xtask check", &[]);
    assert_reads("# cargo xtask check", &[]);
    assert_reads(
        "cargo xtask check  # changed crates only",
        &["cargo xtask check"],
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
    assert_error("cargo xtask", "it names no subcommand");
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
    assert_reads("cargo xtask <command> --help", &[]);
    assert_reads("cargo xtask ...", &[]);
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
    for text in [
        "cargo xtask check 2>&1",
        "cargo xtask check > out.txt",
        "cargo xtask check >>out.txt",
        "cargo xtask check &> out",
        "cargo xtask check < in",
        "(cargo xtask check)",
    ] {
        assert_reads(text, &["cargo xtask check"]);
    }
}

#[test]
fn whatever_comes_before_the_command_is_skipped() {
    assert_reads(
        "(cd x && cargo xtask frobnicate)",
        &["cargo xtask frobnicate"],
    );
    assert_reads("$(cargo xtask check)", &["cargo xtask check"]);
    assert_reads("time env cargo xtask check", &["cargo xtask check"]);
    assert_reads("exec cargo xtask check", &["cargo xtask check"]);
    assert_reads("echo cargo xtask check", &["cargo xtask check"]);
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
        assert_reads(text, &["cargo xtask frobnicate"]);
    }
}

#[test]
fn shell_punctuation_after_a_word_ends_the_command() {
    assert_reads("cargo xtask check &", &["cargo xtask check"]);
    assert_reads("cargo xtask check&", &["cargo xtask check"]);
    assert_reads("$(cargo xtask)", &["cargo xtask"]);
    assert_reads("(cargo +nightly xtask check)", &["cargo xtask check"]);
}

#[test]
fn a_toolchain_after_cargo_is_skipped() {
    assert_reads("cargo +nightly xtask check", &["cargo xtask check"]);
    assert_reads("cargo + xtask check", &[]);
}

#[test]
fn words_are_split_as_a_shell_splits_them() {
    assert_reads("cargo xtask \"check\"", &["cargo xtask check"]);
    assert_reads(
        "FOO=\"a b\" cargo xtask frobnicate",
        &["cargo xtask frobnicate"],
    );
    assert_reads("sudo cargo xtask frobnicate", &["cargo xtask frobnicate"]);
    assert_reads(
        "echo 'a && b; c | d' && cargo xtask check",
        &["cargo xtask check"],
    );
    assert_reads(
        "cargo xtask check # it is fine; really",
        &["cargo xtask check"],
    );
}

#[test]
fn every_command_on_a_line_is_checked_and_a_comment_ends_it() {
    assert_reads(
        "cargo xtask check && cargo xtask setup; cargo xtask frobnicate",
        &[
            "cargo xtask check",
            "cargo xtask setup",
            "cargo xtask frobnicate",
        ],
    );
    assert_reads(
        "cargo xtask check # then cargo xtask frobnicate",
        &["cargo xtask check"],
    );
}

#[test]
fn a_quoted_hash_is_a_word_and_not_a_comment() {
    assert_reads(
        "x \"#a\" cargo xtask frobnicate",
        &["cargo xtask frobnicate"],
    );
}

#[test]
fn cargo_flags_and_a_toolchain_before_xtask_are_skipped() {
    assert_reads(
        "cargo --locked xtask frobnicate",
        &["cargo xtask frobnicate"],
    );
    assert_reads(
        "cargo -q +nightly xtask frobnicate",
        &["cargo xtask frobnicate"],
    );
    assert_reads("cargo --color always xtask x", &[]);
}

/// The lines of a code block with info word `info`, numbered from 1 as `scan` numbers them.
fn block(info: &str, lines: &[&str]) -> Vec<CodeLine> {
    lines
        .iter()
        .enumerate()
        .map(|(index, text)| CodeLine {
            line: 1 + u32::try_from(index).unwrap_or(u32::MAX),
            text: (*text).to_owned(),
            info: info.to_owned(),
        })
        .collect()
}

/// What `read_texts` reads from a block with info word `info` and `lines`, as `(line, text)`.
fn read_of(info: &str, lines: &[&str]) -> Vec<(u32, String)> {
    read_texts(&block(info, lines))
}

/// The violations `problems` reports for a block with info word `info` and `lines`, as
/// `(line, message)`.
fn problems_of(info: &str, lines: &[&str]) -> Vec<(u32, String)> {
    let scan = Scan {
        code: block(info, lines),
        ..Scan::default()
    };
    problems("a.md", &scan)
        .into_iter()
        .map(|found| (found.line, found.message))
        .collect()
}

/// The message for `command`, which does not parse with clap's first line `error`.
fn does_not_parse(command: &str, error: &str) -> String {
    format!("the xtask command `{command}` does not parse: {error}")
}

#[test]
fn a_trailing_backslash_is_dropped_before_the_split() {
    assert_eq!(
        read_of("sh", &["cargo xtask frobnicate \\"]),
        [(1, "cargo xtask frobnicate".to_owned())]
    );
    assert_eq!(
        read_of("sh", &["cargo xtask check\\  "]),
        [(1, "cargo xtask check".to_owned())]
    );
}

#[test]
fn a_continued_line_is_joined_with_the_next_line_of_its_block() {
    assert_eq!(
        read_of("sh", &["cargo xtask \\", "frobnicate"]),
        [(1, "cargo xtask frobnicate".to_owned())]
    );
    assert_eq!(
        read_of("sh", &["cargo xtask \\", "check \\", "--quick"]),
        [(1, "cargo xtask check --quick".to_owned())]
    );
    let apart = [
        CodeLine {
            line: 1,
            text: "cargo xtask check \\".to_owned(),
            info: "sh".to_owned(),
        },
        CodeLine {
            line: 3,
            text: "--quick".to_owned(),
            info: "sh".to_owned(),
        },
    ];
    assert_eq!(
        read_texts(&apart),
        [
            (1, "cargo xtask check".to_owned()),
            (3, "--quick".to_owned())
        ]
    );
}

#[test]
fn each_block_has_its_own_continuation_character() {
    let joined = [(1, "cargo xtask check --quick".to_owned())];
    assert_eq!(
        read_of("powershell", &["cargo xtask check `", "--quick"]),
        joined
    );
    assert_eq!(read_of("pwsh", &["cargo xtask check `", "--quick"]), joined);
    assert_eq!(read_of("cmd", &["cargo xtask check ^", "--quick"]), joined);
    assert_eq!(read_of("bat", &["cargo xtask check ^", "--quick"]), joined);
    let apart = [
        (1, "cargo xtask check \\".to_owned()),
        (2, "--quick".to_owned()),
    ];
    assert_eq!(
        read_of("powershell", &["cargo xtask check \\", "--quick"]),
        apart
    );
    assert_eq!(
        read_of("sh", &["cargo xtask check ^", "--quick"]),
        [
            (1, "cargo xtask check ^".to_owned()),
            (2, "--quick".to_owned()),
        ]
    );
    assert_eq!(
        read_of("text", &["cargo xtask check \\", "--quick"]),
        Vec::new()
    );
}

#[test]
fn a_session_continuation_is_read_after_a_chevron_or_without_one() {
    let joined = [(1, "cargo xtask check --quick".to_owned())];
    assert_eq!(
        read_of("console", &["$ cargo xtask check \\", "> --quick"]),
        joined
    );
    assert_eq!(
        read_of("console", &["$ cargo xtask check \\", "--quick"]),
        joined
    );
    assert_eq!(
        read_of("ps1-session", &["PS> cargo xtask check `", "> --quick"]),
        joined
    );
    assert_eq!(
        read_of("console", &["cargo xtask check \\", "--quick"]),
        Vec::new()
    );
}

#[test]
fn a_continued_command_is_checked_at_its_first_line() {
    let unknown = "error: unrecognized subcommand 'frobnicate'";
    let flag = "error: unexpected argument '--quik' found";
    assert_eq!(
        problems_of("sh", &["cargo xtask \\", "frobnicate"]),
        [(1, does_not_parse("cargo xtask frobnicate", unknown))]
    );
    assert_eq!(
        problems_of("sh", &["cargo xtask check \\", "--quik"]),
        [(1, does_not_parse("cargo xtask check --quik", flag))]
    );
    assert_eq!(
        problems_of("console", &["$ cargo xtask check \\", "> --quik"]),
        [(1, does_not_parse("cargo xtask check --quik", flag))]
    );
    assert_eq!(
        problems_of("powershell", &["cargo xtask check `", "--quik"]),
        [(1, does_not_parse("cargo xtask check --quik", flag))]
    );
    assert_eq!(
        problems_of("cmd", &["cargo xtask check ^", "--quik"]),
        [(1, does_not_parse("cargo xtask check --quik", flag))]
    );
    assert_eq!(
        problems_of("sh", &["cargo xtask check \\", "--quick"]),
        Vec::new()
    );
}

#[test]
fn a_continuation_with_no_line_after_it_is_dropped_and_its_command_checked() {
    assert_eq!(
        problems_of("sh", &["cargo xtask frobnicate \\"]),
        [(
            1,
            does_not_parse(
                "cargo xtask frobnicate",
                "error: unrecognized subcommand 'frobnicate'"
            )
        )]
    );
    assert_eq!(
        problems_of("sh", &["cargo xtask \\"]),
        [(1, does_not_parse("cargo xtask", "it names no subcommand"))]
    );
}

#[test]
fn a_line_shlex_cannot_split_is_split_on_whitespace() {
    assert_reads(
        "Don't run cargo xtask frobnicate",
        &["cargo xtask frobnicate"],
    );
    assert_reads(
        "cargo xtask frobnicate | grep \"it's",
        &["cargo xtask frobnicate"],
    );
    assert_reads(
        "cargo xtask check --quick \"open",
        &["cargo xtask check --quick open"],
    );
}

#[test]
fn the_quotes_on_a_split_word_are_trimmed_so_its_command_parses() {
    for line in [
        "cargo xtask buildtime \"rust\" --runs 3 | grep \"won't",
        "cargo xtask buildtime rust --runs \"3\" && echo \"it's done",
    ] {
        assert_reads(line, &["cargo xtask buildtime rust --runs 3"]);
        assert!(
            commands_in(line)
                .iter()
                .all(|command| problem("a.md", 1, command).is_none()),
            "{line}"
        );
    }
}

#[test]
fn a_glued_second_command_is_not_read() {
    assert_reads("a;cargo xtask x", &[]);
}

#[test]
fn a_block_with_no_info_string_or_a_shell_one_reads_every_line() {
    assert_read(
        "",
        "Run cargo xtask check, then sudo cargo xtask setup",
        Some("Run cargo xtask check, then sudo cargo xtask setup"),
    );
    assert_read(
        "Bash",
        "sudo cargo xtask check",
        Some("sudo cargo xtask check"),
    );
    assert_read("rust", "cargo xtask check", None);
    assert_read("md", "Run cargo xtask to see the commands.", None);
}

#[test]
fn a_session_block_reads_only_the_prompted_lines() {
    assert_read("console", "$ cargo xtask check", Some("cargo xtask check"));
    assert_read(
        "Terminal",
        "  % cargo +nightly xtask check",
        Some("cargo +nightly xtask check"),
    );
    assert_read(
        "console",
        "PS> cargo xtask check",
        Some("cargo xtask check"),
    );
    assert_read("console", "# then run cargo xtask check again", None);
    assert_read("console", "> cargo xtask check passes now", None);
    assert_read("console", "PS C:\\repo> cargo xtask check", None);
    assert_read("console", "error: run cargo xtask setup first", None);
    assert!(is_one_of("Console", &SESSIONS));
    assert!(is_one_of("PWSH", &SHELLS));
    assert!(!is_one_of("rust", &SHELLS));
}
