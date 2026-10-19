//! The planted cases of the docs check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

/// The text of the violation for the relative link `$link` in `$file` at `$line`, a path from the
/// repo root that does not exist.
macro_rules! bad_link {
    ($file:literal, $line:literal, $link:literal) => {
        concat!(
            $file,
            ":",
            $line,
            ": the relative link `",
            $link,
            "` does not resolve: `",
            $link,
            "` does not exist\n    fix: create `",
            $link,
            "`, or correct or remove the link"
        )
    };
}

/// The text of the violation for the command `$command` at `GUIDE.md:$line`, which does not
/// parse for the reason `$detail`.
macro_rules! bad_command {
    ($line:literal, $command:literal, $detail:literal) => {
        concat!(
            "GUIDE.md:",
            $line,
            ": the xtask command `",
            $command,
            "` does not parse: ",
            $detail,
            "\n    fix: ",
            crate::checks::docs::commands::command_fix!()
        )
    };
}

/// The text of the violation for the backticked path `$path` at `GUIDE.md:$line`, which does not
/// exist.
macro_rules! bad_path {
    ($line:literal, $path:literal) => {
        concat!(
            "GUIDE.md:",
            $line,
            ": `",
            $path,
            "` does not exist\n    fix: create `",
            $path,
            "`, or correct or remove the reference"
        )
    };
}

crate::checks::planted::cases! {
    /// A backticked repo path that does not exist fails at the span's line.
    missing_backticked_path: FailsWith(&[
        bad_path!(1, "crates/nope/src/lib.rs"),
    ]) => super::cases::missing_backticked_path,
    /// A relative link to a missing file fails, naming the file it resolves to.
    broken_relative_link: FailsWith(&[
        bad_link!("GUIDE.md", 1, "notes.md"),
    ]) => super::cases::broken_relative_link,
    /// The anchor is cut before the path is resolved, so `nope.md#x` fails on `nope.md`.
    missing_file_with_anchor: FailsWith(&[
        bad_link!("GUIDE.md", 1, "nope.md"),
    ]) => super::cases::missing_file_with_anchor,
    /// A query is cut too: `docs/real.md?plain=1` passes and `nope.md?plain=1` fails on `nope.md`.
    query_is_cut_before_the_lookup: FailsWith(&[
        bad_link!("GUIDE.md", 2, "nope.md"),
    ]) => super::cases::query_is_cut_before_the_lookup,
    /// A reference definition is a link too, and fails at its own line.
    broken_reference_definition: FailsWith(&[
        bad_link!("GUIDE.md", 3, "gone.md"),
    ]) => super::cases::broken_reference_definition,
    /// A relative link that climbs out of the repo, other than into the private folder, fails.
    link_leaves_the_repo: FailsWith(&[
        "GUIDE.md:1: the relative link `../../outside.md` leaves the repo\n    \
         fix: point it at a file inside the repo, or remove it",
    ]) => super::cases::link_leaves_the_repo,
    /// `crates/../../x` fails as a backticked path, for its `..`, and as a link, which leaves the
    /// repo, whatever exists on disk above the repo.
    dot_dot_path_and_link_fail: FailsWith(&[
        "GUIDE.md:1: `crates/../../x` has a `..` part\n    \
         fix: write the path from the repo root, without `..`",
        "GUIDE.md:1: the relative link `crates/../../x` leaves the repo\n    \
         fix: point it at a file inside the repo, or remove it",
    ]) => super::cases::dot_dot_path_and_link_fail,
    /// A link and a path that differ from the real file only in case fail, as on Linux and GitHub.
    case_mismatch_fails: FailsWith(&[
        bad_path!(1, "docs/REAL.md"),
        bad_link!("GUIDE.md", 1, "docs/REAL.md"),
    ]) => super::cases::case_mismatch_fails,
    /// A file on disk that git does not list (an ignored local file) does not exist for the docs.
    unlisted_local_file_fails: FailsWith(&[
        bad_path!(1, "docs/local.md"),
        bad_link!("GUIDE.md", 1, "local.md"),
    ]) => super::cases::unlisted_local_file_fails,
    /// The same broken link twice on one line, around another, is reported once.
    repeats_on_one_line_are_reported_once: FailsWith(&[
        bad_link!("GUIDE.md", 1, "gone.md"),
        bad_link!("GUIDE.md", 1, "nope.md"),
    ]) => super::cases::repeats_on_one_line_are_reported_once,
    /// A code span inside an HTML comment counts like any other.
    path_in_html_comment: FailsWith(&[
        bad_path!(1, "crates/gone/"),
    ]) => super::cases::path_in_html_comment,
    /// A code span in a comment inside a paragraph counts too.
    path_in_inline_html_comment: FailsWith(&[
        bad_path!(1, "crates/gone/"),
    ]) => super::cases::path_in_inline_html_comment,
    /// A code span on the third line of a comment fails at line 3.
    path_in_multiline_html_comment: FailsWith(&[
        bad_path!(3, "crates/gone/"),
    ]) => super::cases::path_in_multiline_html_comment,
    /// A crate path whose row is not in the Crates table is not exempt.
    unplanned_crate_path: FailsWith(&[
        bad_path!(1, "crates/store/src/lib.rs"),
    ]) => super::cases::unplanned_crate_path,
    /// A `<placeholder>` in a path is not a path: `crates/<name>/`.
    path_with_angle_brackets_passes: Passes => super::cases::path_with_angle_brackets_passes,
    /// A `<` alone makes a span a pattern too.
    path_with_an_open_angle_passes: Passes => super::cases::path_with_an_open_angle_passes,
    /// A `>` alone makes a span a pattern too.
    path_with_a_close_angle_passes: Passes => super::cases::path_with_a_close_angle_passes,
    /// A glob is not a path: `xtask/**`, `docs/*.md`.
    path_with_a_star_passes: Passes => super::cases::path_with_a_star_passes,
    /// A `?` wildcard is not a path.
    path_with_a_question_mark_passes: Passes => super::cases::path_with_a_question_mark_passes,
    /// A brace list is not a path: `ui/{app,components}`.
    path_with_a_brace_passes: Passes => super::cases::path_with_a_brace_passes,
    /// An ellipsis is not a path: `crates/...`.
    path_with_an_ellipsis_passes: Passes => super::cases::path_with_an_ellipsis_passes,
    /// A trailing `:<line>` and `:<line>:<col>` are cut before the lookup.
    path_with_a_line_and_column_passes: Passes => super::cases::path_with_a_line_and_column_passes,
    /// After the `:<line>` is cut, a missing file still fails, named without the line.
    missing_path_with_a_line_fails: FailsWith(&[
        bad_path!(1, "crates/gone.rs"),
    ]) => super::cases::missing_path_with_a_line_fails,
    /// An anchor is cut before the lookup: `xtask/src/main.rs#sec`.
    path_with_an_anchor_passes: Passes => super::cases::path_with_an_anchor_passes,
    /// A line range is cut like a line: `xtask/src/main.rs:1-3`.
    path_with_a_line_range_passes: Passes => super::cases::path_with_a_line_range_passes,
    /// Trailing `,`, `.`, `;` and `:` are punctuation, not part of the path.
    path_with_trailing_punctuation_passes: Passes => super::cases::path_with_trailing_punctuation_passes,
    /// A span with whitespace is prose, not a path: `crates/gone.rs (line 3)`.
    span_with_whitespace_passes: Passes => super::cases::span_with_whitespace_passes,
    /// A top folder in the wrong case is still a repo path, and fails the exact lookup.
    wrong_case_top_folder_fails: FailsWith(&[
        bad_path!(1, "Xtask/src/main.rs"),
    ]) => super::cases::wrong_case_top_folder_fails,
    /// A path marked `(planned)` that does not exist passes.
    planned_missing_path_passes: Passes => super::cases::planned_missing_path_passes,
    /// A path marked `(planned)` that exists fails: the marker is dropped when the path lands.
    planned_existing_path_fails: FailsWith(&[
        "GUIDE.md:1: `xtask/src/main.rs` exists, but the text marks it (planned)\n    \
         fix: drop the (planned) after `xtask/src/main.rs`",
    ]) => super::cases::planned_existing_path_fails,
    /// The same missing path without the marker still fails.
    unmarked_missing_path_fails: FailsWith(&[
        bad_path!(1, "ui/components"),
    ]) => super::cases::unmarked_missing_path_fails,
    /// An `xtask` subcommand that clap does not know fails at its line, with clap's first line.
    unknown_xtask_subcommand: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::unknown_xtask_subcommand,
    /// An unknown flag fails, as an unknown subcommand does.
    unknown_xtask_flag: FailsWith(&[
        bad_command!(1, "cargo xtask check --frobnicate", "error: unexpected argument '--frobnicate' found"),
    ]) => super::cases::unknown_xtask_flag,
    /// A fenced line that starts with `cargo xtask` is checked the same way.
    unknown_xtask_command_in_fence: FailsWith(&[
        bad_command!(2, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::unknown_xtask_command_in_fence,
    /// A tab-indented code line is code: its command is checked and its link is not read.
    tab_indented_code_command_fails: FailsWith(&[
        bad_command!(3, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::tab_indented_code_command_fails,
    /// A fence inside a quote inside a list item holds code, so its link is not read.
    fence_in_quote_in_list_item_hides_its_contents: Passes => super::cases::fence_in_quote_in_list_item_hides_its_contents,
    /// A span that is exactly `cargo xtask` names the tool in prose.
    bare_tool_span_passes: Passes => super::cases::bare_tool_span_passes,
    /// The same words as a line of code are a command that names no subcommand.
    bare_tool_in_code_fails: FailsWith(&[
        bad_command!(2, "cargo xtask", "it names no subcommand"),
    ]) => super::cases::bare_tool_in_code_fails,
    /// `cargo xtask <command> --help`, the check's own fix text, is not read: the subcommand is a
    /// placeholder.
    placeholder_subcommand_passes: Passes => super::cases::placeholder_subcommand_passes,
    /// `cargo xtask ...` is not read either: `...` is a placeholder.
    ellipsis_subcommand_passes: Passes => super::cases::ellipsis_subcommand_passes,
    /// A placeholder for a flag's value passes: the words before it parse but for that value.
    placeholder_flag_value_passes: Passes => super::cases::placeholder_flag_value_passes,
    /// The words before a placeholder are still checked.
    unknown_subcommand_before_a_placeholder_fails: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::unknown_subcommand_before_a_placeholder_fails,
    /// A `+<toolchain>` after `cargo` does not hide the command.
    toolchain_command_fails: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::toolchain_command_fails,
    /// `AGENTS.md` at 121 lines fails at line 121, the first line over the limit.
    agents_121_lines: FailsWith(&[
        "AGENTS.md:121: AGENTS.md has 121 lines, over the limit of 120\n    \
         fix: cut AGENTS.md to 120 lines, moving what you cut into the file that owns it",
    ]) => super::cases::agents_121_lines,
    /// `AGENTS.md` at 8193 bytes fails at the line of the 8193rd byte.
    agents_8193_bytes: FailsWith(&[
        "AGENTS.md:1: AGENTS.md is 8193 bytes, over the limit of 8192\n    \
         fix: cut AGENTS.md to 8192 bytes, moving what you cut into the file that owns it",
    ]) => super::cases::agents_8193_bytes,
    /// `ARCHITECTURE.md` at 301 lines fails at line 301.
    architecture_301_lines: FailsWith(&[
        "ARCHITECTURE.md:301: ARCHITECTURE.md has 301 lines, over the limit of 300\n    \
         fix: cut ARCHITECTURE.md to 300 lines, moving what you cut into the file that owns it",
    ]) => super::cases::architecture_301_lines,
    /// `CLAUDE.md` with anything after `@AGENTS.md` fails at line 2.
    claude_extra_line: FailsWith(&[
        "CLAUDE.md:2: CLAUDE.md must hold only `@AGENTS.md`\n    \
         fix: move what CLAUDE.md holds besides `@AGENTS.md` into AGENTS.md, then keep only `@AGENTS.md` in CLAUDE.md",
    ]) => super::cases::claude_extra_line,
    /// An `<a href>` to a missing file fails at the tag's line.
    html_link_to_a_missing_file_fails: FailsWith(&[
        bad_link!("GUIDE.md", 1, "nope.md"),
    ]) => super::cases::html_link_to_a_missing_file_fails,
    /// An `<img src>` to a missing file fails, with the quotes around it removed.
    html_image_to_a_missing_file_fails: FailsWith(&[
        bad_link!("GUIDE.md", 1, "nope.png"),
    ]) => super::cases::html_image_to_a_missing_file_fails,
    /// A `$ ` prompt before `cargo xtask` in a fence does not hide the command.
    prompt_command_in_fence_fails: FailsWith(&[
        bad_command!(2, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::prompt_command_in_fence_fails,
    /// `VAR=value` before `cargo xtask` in a span does not hide the command.
    env_assignment_command_in_span_fails: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::env_assignment_command_in_span_fails,
    /// A command after `&&` in a span is checked on its own.
    chained_command_in_span_fails: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::chained_command_in_span_fails,
    /// An indented code block line that starts a command is checked.
    indented_code_command_fails: FailsWith(&[
        bad_command!(3, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::indented_code_command_fails,
    /// `--help` does not hide an unknown subcommand after it.
    help_with_an_unknown_subcommand_fails: FailsWith(&[
        bad_command!(1, "cargo xtask --help frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::help_with_an_unknown_subcommand_fails,
    /// `--help` does not hide an unknown flag after it.
    help_with_an_unknown_flag_fails: FailsWith(&[
        bad_command!(1, "cargo xtask check --help --frobnicate", "error: unexpected argument '--frobnicate' found"),
    ]) => super::cases::help_with_an_unknown_flag_fails,
    /// A `.MD` file is a Markdown file, and its links are checked.
    uppercase_md_extension_is_checked: FailsWith(&[
        bad_link!("GUIDE.MD", 1, "nope.md"),
    ]) => super::cases::uppercase_md_extension_is_checked,
    /// A `.markdown` file is a Markdown file, and its links are checked.
    markdown_extension_is_checked: FailsWith(&[
        bad_link!("guide.markdown", 1, "nope.md"),
    ]) => super::cases::markdown_extension_is_checked,
    /// A stderr redirection after a command is shell syntax, not an argument.
    stderr_redirect_passes: Passes => super::cases::stderr_redirect_passes,
    /// A stdout redirection after a command is shell syntax, not an argument.
    stdout_redirect_passes: Passes => super::cases::stdout_redirect_passes,
    /// A command in a subshell is read, and its closing parenthesis is not an argument.
    subshell_with_valid_command_passes: Passes => super::cases::subshell_with_valid_command_passes,
    /// A command in a subshell is checked, and fails on its unknown subcommand.
    subshell_with_unknown_subcommand_fails: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::subshell_with_unknown_subcommand_fails,
    /// A command in a command substitution is checked.
    command_substitution_fails: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::command_substitution_fails,
    /// A `time` and `env` before the command are skipped, and the command is checked.
    time_and_env_prefix_fails: FailsWith(&[
        bad_command!(1, "cargo xtask frobnicate", "error: unrecognized subcommand 'frobnicate'"),
    ]) => super::cases::time_and_env_prefix_fails,
    /// A repo without `AGENTS.md` fails at line 1, the whole-file line.
    missing_agents: FailsWith(&[
        "AGENTS.md:1: AGENTS.md does not exist\n    \
         fix: create AGENTS.md at the repo root, or restore it from git history",
    ]) => super::cases::missing_agents,
    /// A Markdown file with the links, paths and commands it must have passes.
    clean_docs_pass: Passes => super::cases::clean_docs_pass,
    /// `AGENTS.md` at exactly 120 lines and 8192 bytes passes.
    agents_at_120_lines_and_8192_bytes_passes: Passes => super::cases::agents_at_120_lines_and_8192_bytes_passes,
    /// `CLAUDE.md` without a final newline is still exactly `@AGENTS.md`.
    claude_without_newline_passes: Passes => super::cases::claude_without_newline_passes,
    /// No Markdown file at all: the check looked and found none, so the zero line gets the normal
    /// fix, not the stopped one.
    no_markdown_and_no_required_files: FailsWith(&[
        "AGENTS.md:1: AGENTS.md does not exist\n    \
         fix: create AGENTS.md at the repo root, or restore it from git history",
        "ARCHITECTURE.md:1: ARCHITECTURE.md does not exist\n    \
         fix: create ARCHITECTURE.md at the repo root, or restore it from git history",
        "CLAUDE.md:1: CLAUDE.md does not exist\n    \
         fix: create CLAUDE.md holding only `@AGENTS.md`, or restore it from git history",
        "0 Markdown files inspected in the repo's git-tracked or unignored .md and .markdown files\n    \
         fix: docs looks for Markdown files in the repo's git-tracked or unignored .md and \
         .markdown files; restore them, or, if they moved, update the consts \
         xtask/src/checks/docs.rs builds that place from",
    ]) => super::cases::no_markdown_and_no_required_files,
}
