//! The planted cases of the docs check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). Their builders are in `tests/repos.rs`, and the lines
//! several cases share are functions in `tests.rs`.

use crate::checks::docs::Fix;
use crate::checks::planted::fails;

crate::checks::planted::cases! {
    /// A backticked repo path that does not exist fails at the span's line.
    missing_backticked_path: Fails[super::bad_path(1, "crates/nope/src/lib.rs")],
        inspects 4 => super::repos::missing_backticked_path,
    /// A relative link to a missing file fails, naming the file it resolves to.
    broken_relative_link: Fails[super::bad_link("GUIDE.md", 1, "notes.md")],
        inspects 4 => super::repos::broken_relative_link,
    /// The anchor is cut before the path is resolved, so `nope.md#x` fails on `nope.md`.
    missing_file_with_anchor: Fails[super::bad_link("GUIDE.md", 1, "nope.md")],
        inspects 4 => super::repos::missing_file_with_anchor,
    /// A query is cut too: `docs/real.md?plain=1` passes and `nope.md?plain=1` fails on `nope.md`.
    query_is_cut_before_the_lookup: Fails[super::bad_link("GUIDE.md", 2, "nope.md")],
        inspects 5 => super::repos::query_is_cut_before_the_lookup,
    /// A reference definition is a link too, and fails at its own line.
    broken_reference_definition: Fails[super::bad_link("GUIDE.md", 3, "gone.md")],
        inspects 4 => super::repos::broken_reference_definition,
    /// A relative link that climbs out of the repo, other than into the private folder, fails.
    link_leaves_the_repo: Fails[fails(
        "GUIDE.md:1",
        "the relative link `../../outside.md` leaves the repo",
        Fix::PointInside,
    )], inspects 4 => super::repos::link_leaves_the_repo,
    /// `crates/../../x` fails as a backticked path, for its `..`, and as a link, which leaves the
    /// repo, whatever exists on disk above the repo.
    dot_dot_path_and_link_fail: Fails[
        fails("GUIDE.md:1", "`crates/../../x` has a `..` part", Fix::NoDotParts),
        fails(
            "GUIDE.md:1",
            "the relative link `crates/../../x` leaves the repo",
            Fix::PointInside,
        ),
    ], inspects 4 => super::repos::dot_dot_path_and_link_fail,
    /// A link and a path that differ from the real file only in case fail, as on Linux and GitHub.
    case_mismatch_fails: Fails[
        super::bad_path(1, "docs/REAL.md"),
        super::bad_link("GUIDE.md", 1, "docs/REAL.md"),
    ], inspects 5 => super::repos::case_mismatch_fails,
    /// A file on disk that git does not list (an ignored local file) does not exist for the docs,
    /// and the message says git ignores it rather than asking to create it.
    unlisted_local_file_fails: Fails[
        fails(
            "GUIDE.md:1",
            "`docs/local.md` is on disk, but git ignores it",
            Fix::NameTrackedFile,
        ),
        fails(
            "GUIDE.md:1",
            "the relative link `local.md` does not resolve: `local.md` is on disk, but \
             git ignores it",
            Fix::LinkTrackedFile,
        ),
    ], inspects 4 => super::repos::unlisted_local_file_fails,
    /// The same broken link twice on one line, around another, is reported once.
    repeats_on_one_line_are_reported_once: Fails[
        super::bad_link("GUIDE.md", 1, "gone.md"),
        super::bad_link("GUIDE.md", 1, "nope.md"),
    ], inspects 4 => super::repos::repeats_on_one_line_are_reported_once,
    /// A code span inside an HTML comment counts like any other.
    path_in_html_comment: Fails[super::bad_path(1, "crates/gone/")],
        inspects 4 => super::repos::path_in_html_comment,
    /// A code span in a comment inside a paragraph counts too.
    path_in_inline_html_comment: Fails[super::bad_path(1, "crates/gone/")],
        inspects 4 => super::repos::path_in_inline_html_comment,
    /// A code span on the third line of a comment fails at line 3.
    path_in_multiline_html_comment: Fails[super::bad_path(3, "crates/gone/")],
        inspects 4 => super::repos::path_in_multiline_html_comment,
    /// A crate path whose row is not in the Crates table is not exempt.
    unplanned_crate_path: Fails[super::bad_path(1, "crates/store/src/lib.rs")],
        inspects 4 => super::repos::unplanned_crate_path,
    /// A `<placeholder>` in a path is not a path: `crates/<name>/`.
    path_with_angle_brackets_passes: Passes,
        inspects 4 => super::repos::path_with_angle_brackets_passes,
    /// A `<` alone makes a span a pattern too.
    path_with_an_open_angle_passes: Passes,
        inspects 4 => super::repos::path_with_an_open_angle_passes,
    /// A `>` alone makes a span a pattern too.
    path_with_a_close_angle_passes: Passes,
        inspects 4 => super::repos::path_with_a_close_angle_passes,
    /// A glob is not a path: `xtask/**`, `docs/*.md`.
    path_with_a_star_passes: Passes, inspects 4 => super::repos::path_with_a_star_passes,
    /// A `?` wildcard is not a path.
    path_with_a_question_mark_passes: Passes,
        inspects 4 => super::repos::path_with_a_question_mark_passes,
    /// A brace list is not a path: `ui/{app,components}`.
    path_with_a_brace_passes: Passes, inspects 4 => super::repos::path_with_a_brace_passes,
    /// An ellipsis is not a path: `crates/...`.
    path_with_an_ellipsis_passes: Passes,
        inspects 4 => super::repos::path_with_an_ellipsis_passes,
    /// A trailing `:<line>` is cut before the lookup.
    path_with_a_line_passes: Passes, inspects 4 => super::repos::path_with_a_line_passes,
    /// After the `:<line>` is cut, a missing file still fails, named without the line.
    missing_path_with_a_line_fails: Fails[super::bad_path(1, "crates/gone.rs")],
        inspects 4 => super::repos::missing_path_with_a_line_fails,
    /// An anchor is cut before the lookup: `xtask/src/main.rs#sec`.
    path_with_an_anchor_passes: Passes,
        inspects 4 => super::repos::path_with_an_anchor_passes,
    /// Trailing `,`, `.`, `;` and `:` are punctuation, not part of the path.
    path_with_trailing_punctuation_passes: Passes,
        inspects 4 => super::repos::path_with_trailing_punctuation_passes,
    /// The first word of a span is the path: `crates/nope/lib.rs and more` and
    /// `crates/nope.rs --flag` fail, `crates/host/src/main.rs --help` passes.
    first_word_is_the_path: Fails[
        super::bad_path(1, "crates/nope.rs"),
        super::bad_path(1, "crates/nope/lib.rs"),
    ], inspects 4 => super::repos::first_word_is_the_path,
    /// A first word that is a pattern is not read: `crates/<name>/src and more`.
    first_word_pattern_passes: Passes,
        inspects 4 => super::repos::first_word_pattern_passes,
    /// A top folder in the wrong case is still a repo path, and fails the exact lookup.
    wrong_case_top_folder_fails: Fails[super::bad_path(1, "Xtask/src/main.rs")],
        inspects 4 => super::repos::wrong_case_top_folder_fails,
    /// A trailing `..` is a `..` part, not punctuation: `crates/a/..` and `crates/..` fail.
    trailing_dot_dot_fails: Fails[
        fails("GUIDE.md:1", "`crates/..` has a `..` part", Fix::NoDotParts),
        fails("GUIDE.md:1", "`crates/a/..` has a `..` part", Fix::NoDotParts),
    ], inspects 4 => super::repos::trailing_dot_dot_fails,
    /// In `ARCHITECTURE.md`, a path marked `(planned)` that does not exist passes.
    planned_missing_path_passes: Passes,
        inspects 4 => super::repos::planned_missing_path_passes,
    /// In `ARCHITECTURE.md`, a path marked `(planned)` that exists fails: the marker is dropped
    /// when the path lands.
    planned_existing_path_fails: Fails[fails(
        "ARCHITECTURE.md:10",
        "`xtask/src/main.rs` exists, but the text marks it (planned)",
        Fix::DropPlanned {
            path: "xtask/src/main.rs".to_owned(),
        },
    )], inspects 4 => super::repos::planned_existing_path_fails,
    /// In `ARCHITECTURE.md`, a marked path that differs from a listed one only in case fails,
    /// naming the listed one.
    planned_case_variant_fails: Fails[fails(
        "ARCHITECTURE.md:10",
        "`docs/REAL.md` is marked (planned), but `docs/real.md` exists, in another case",
        Fix::WriteCase {
            real: "docs/real.md".to_owned(),
        },
    )], inspects 4 => super::repos::planned_case_variant_fails,
    /// In `ARCHITECTURE.md`, a marked path whose folder differs from a listed folder only in case
    /// fails, naming the listed folder.
    planned_folder_case_variant_fails: Fails[fails(
        "ARCHITECTURE.md:10",
        "`Docs/new.md` is marked (planned), but its folder `Docs` exists as `docs`",
        Fix::WriteUnderFolder {
            real: "docs".to_owned(),
        },
    )], inspects 4 => super::repos::planned_folder_case_variant_fails,
    /// Outside `ARCHITECTURE.md` the marker is not read: Rohan's three marked spans fail.
    planned_marker_outside_architecture_fails: Fails[
        super::bad_path(1, "crates/nope/lib.rs"),
        super::bad_path(1, "docs/REAL.md"),
        super::bad_path(1, "ui/componnets"),
    ], inspects 5 => super::repos::planned_marker_outside_architecture_fails,
    /// The same missing path without the marker still fails.
    unmarked_missing_path_fails: Fails[super::bad_path(1, "ui/components")],
        inspects 4 => super::repos::unmarked_missing_path_fails,
    /// An `xtask` subcommand that clap does not know fails at its line, with clap's first line.
    unknown_xtask_subcommand: Fails[super::bad_command(
        1,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::unknown_xtask_subcommand,
    /// A fenced line that starts with `cargo xtask` is checked the same way.
    unknown_xtask_command_in_fence: Fails[super::bad_command(
        2,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::unknown_xtask_command_in_fence,
    /// A tab-indented code line is code: its command is checked and its link is not read.
    tab_indented_code_command_fails: Fails[super::bad_command(
        3,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::tab_indented_code_command_fails,
    /// A fence inside a quote inside a list item holds code, so its link is not read.
    fence_in_quote_in_list_item_hides_its_contents: Passes,
        inspects 4 => super::repos::fence_in_quote_in_list_item_hides_its_contents,
    /// `AGENTS.md` at 121 lines fails at line 121, the first line over the limit.
    agents_121_lines: Fails[fails(
        "AGENTS.md:121",
        "AGENTS.md has 121 lines, over the limit of 120",
        Fix::CutLines {
            file: "AGENTS.md".to_owned(),
            limit: 120,
        },
    )], inspects 3 => super::repos::agents_121_lines,
    /// `AGENTS.md` at 8193 bytes fails at the line of the 8193rd byte.
    agents_8193_bytes: Fails[fails(
        "AGENTS.md:1",
        "AGENTS.md is 8193 bytes, over the limit of 8192",
        Fix::CutBytes {
            file: "AGENTS.md".to_owned(),
            limit: 8192,
        },
    )], inspects 3 => super::repos::agents_8193_bytes,
    /// `ARCHITECTURE.md` at 301 lines fails at line 301.
    architecture_301_lines: Fails[fails(
        "ARCHITECTURE.md:301",
        "ARCHITECTURE.md has 301 lines, over the limit of 300",
        Fix::CutLines {
            file: "ARCHITECTURE.md".to_owned(),
            limit: 300,
        },
    )], inspects 3 => super::repos::architecture_301_lines,
    /// `AGENTS.md` at 121 lines ended by lone `\r`, which GitHub shows as lines, fails too.
    agents_121_lone_cr_lines: Fails[fails(
        "AGENTS.md:121",
        "AGENTS.md has 121 lines, over the limit of 120",
        Fix::CutLines {
            file: "AGENTS.md".to_owned(),
            limit: 120,
        },
    )], inspects 3 => super::repos::agents_121_lone_cr_lines,
    /// `ARCHITECTURE.md`, which has no byte limit, at 301 lines ended by lone `\r` fails.
    architecture_301_lone_cr_lines: Fails[fails(
        "ARCHITECTURE.md:301",
        "ARCHITECTURE.md has 301 lines, over the limit of 300",
        Fix::CutLines {
            file: "ARCHITECTURE.md".to_owned(),
            limit: 300,
        },
    )], inspects 3 => super::repos::architecture_301_lone_cr_lines,
    /// `CLAUDE.md` with anything after `@AGENTS.md` fails at line 2.
    claude_extra_line: Fails[fails(
        "CLAUDE.md:2",
        "CLAUDE.md must hold only `@AGENTS.md`",
        Fix::MoveImport {
            file: "CLAUDE.md".to_owned(),
            target: "AGENTS.md".to_owned(),
        },
    )], inspects 3 => super::repos::claude_extra_line,
    /// An `<a href>` to a missing file fails at the tag's line.
    html_link_to_a_missing_file_fails: Fails[super::bad_link("GUIDE.md", 1, "nope.md")],
        inspects 4 => super::repos::html_link_to_a_missing_file_fails,
    /// An `<img src>` to a missing file fails, with the quotes around it removed.
    html_image_to_a_missing_file_fails: Fails[super::bad_link("GUIDE.md", 1, "nope.png")],
        inspects 4 => super::repos::html_image_to_a_missing_file_fails,
    /// A `\` that ends a line joins it with the next one, so `cargo xtask \` and `frobnicate` fail
    /// as one command, reported at the first line.
    continued_command_fails: Fails[super::bad_command(
        2,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::continued_command_fails,
    /// An indented code block line that starts a command is checked.
    indented_code_command_fails: Fails[super::bad_command(
        3,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::indented_code_command_fails,
    /// A `.MD` file is a Markdown file, and its links are checked.
    uppercase_md_extension_is_checked: Fails[super::bad_link("GUIDE.MD", 1, "nope.md")],
        inspects 4 => super::repos::uppercase_md_extension_is_checked,
    /// A `.markdown` file is a Markdown file, and its links are checked.
    markdown_extension_is_checked: Fails[super::bad_link("guide.markdown", 1, "nope.md")],
        inspects 4 => super::repos::markdown_extension_is_checked,
    /// Only shell blocks (no info string, or a shell one) and the prompted lines of session blocks
    /// are read: the prose lines in rust, console output, markdown and yaml pass.
    prose_in_code_blocks_passes: Passes, inspects 4 => super::repos::prose_in_code_blocks_passes,
    /// A `# ` comment, a `> ` quoted line and output in a console block are not commands.
    output_and_comments_in_sessions_pass: Passes,
        inspects 4 => super::repos::output_and_comments_in_sessions_pass,
    /// A `text` or `text,ignore` block holds output, so a failing command shown in it is not read.
    text_block_output_passes: Passes, inspects 4 => super::repos::text_block_output_passes,
    /// A block with no info string is read line by line, as a shell block is, so prose that says
    /// `cargo xtask to` fails.
    plain_block_prose_fails: Fails[super::bad_command(
        2,
        "cargo xtask to see the commands.",
        "error: unrecognized subcommand 'to'",
    )], inspects 4 => super::repos::plain_block_prose_fails,
    /// A repo without `AGENTS.md` fails at line 1, the whole-file line.
    missing_agents: Fails[fails(
        "AGENTS.md:1",
        "AGENTS.md does not exist",
        Fix::CreateRequired {
            file: "AGENTS.md".to_owned(),
        },
    )], inspects 2 => super::repos::missing_agents,
    /// A Markdown file with the links, paths and commands it must have passes.
    clean_docs_pass: Passes, inspects 6 => super::repos::clean_docs_pass,
    /// `AGENTS.md` at exactly 120 lines and 8192 bytes passes.
    agents_at_120_lines_and_8192_bytes_passes: Passes,
        inspects 3 => super::repos::agents_at_120_lines_and_8192_bytes_passes,
    /// `CLAUDE.md` without a final newline is still exactly `@AGENTS.md`.
    claude_without_newline_passes: Passes,
        inspects 3 => super::repos::claude_without_newline_passes,
    /// No Markdown file at all: the check looked and found none, so the zero line gets the normal
    /// fix, not the stopped one.
    no_markdown_and_no_required_files: Fails[
        fails(
            "AGENTS.md:1",
            "AGENTS.md does not exist",
            Fix::CreateRequired {
                file: "AGENTS.md".to_owned(),
            },
        ),
        fails(
            "ARCHITECTURE.md:1",
            "ARCHITECTURE.md does not exist",
            Fix::CreateRequired {
                file: "ARCHITECTURE.md".to_owned(),
            },
        ),
        fails(
            "CLAUDE.md:1",
            "CLAUDE.md does not exist",
            Fix::CreateImport {
                file: "CLAUDE.md".to_owned(),
                target: "AGENTS.md".to_owned(),
            },
        ),
        super::no_markdown_inspected(),
    ], inspects 0 => super::repos::no_markdown_and_no_required_files,
    /// A link from the repo root (`/nope.md`) is looked up from the root, and fails.
    root_slash_link_fails: Fails[fails(
        "GUIDE.md:1",
        "the relative link `/nope.md` does not resolve: `nope.md` does not exist",
        Fix::CreateOrCorrectLink {
            path: "nope.md".to_owned(),
        },
    )], inspects 4 => super::repos::root_slash_link_fails,
    /// A link to the private folder beside the repo is not read: outside contributors lack it.
    private_folder_link_passes: Passes, inspects 4 => super::repos::private_folder_link_passes,
    /// A link that names the private folder, then climbs out of it, leaves the repo.
    private_folder_then_climb_fails: Fails[fails(
        "GUIDE.md:1",
        "the relative link `../clauseo-acp-private/../outside.md` leaves the repo",
        Fix::PointInside,
    )], inspects 4 => super::repos::private_folder_then_climb_fails,
    /// A Markdown image to a missing file fails like a link.
    markdown_image_to_a_missing_file_fails: Fails[super::bad_link("GUIDE.md", 1, "nope.png")],
        inspects 4 => super::repos::markdown_image_to_a_missing_file_fails,
    /// A command after a `$` prompt and an assignment is read from `cargo`.
    command_after_a_prompt_and_assignment_fails: Fails[super::bad_command(
        2,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::command_after_a_prompt_and_assignment_fails,
    /// A toolchain after `cargo` (`+nightly`) is skipped, and the command still checked.
    command_with_a_toolchain_fails: Fails[super::bad_command(
        1,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::command_with_a_toolchain_fails,
    /// Cargo's own flags before `xtask` are skipped, and the command still checked.
    command_after_cargo_flags_fails: Fails[super::bad_command(
        1,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::command_after_cargo_flags_fails,
    /// The second command on a line, after `&&`, is checked too.
    second_command_on_a_line_fails: Fails[super::bad_command(
        1,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::second_command_on_a_line_fails,
    /// A flag the subcommand does not have fails.
    unknown_flag_fails: Fails[super::bad_command(
        1,
        "cargo xtask check --frobnicate",
        "error: unexpected argument '--frobnicate' found",
    )], inspects 4 => super::repos::unknown_flag_fails,
    /// A value outside the flag's range fails.
    value_outside_its_range_fails: Fails[super::bad_command(
        1,
        "cargo xtask buildtime --runs 0",
        "error: invalid value '0' for '--runs <RUNS>': 0 is not in 1..=4294967295",
    )], inspects 4 => super::repos::value_outside_its_range_fails,
    /// `--help` before an unknown subcommand does not make it parse.
    unknown_subcommand_after_help_fails: Fails[super::bad_command(
        1,
        "cargo xtask --help frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::unknown_subcommand_after_help_fails,
    /// A quoted subcommand is split as a shell splits it, and fails.
    quoted_subcommand_fails: Fails[super::bad_command(
        2,
        "cargo xtask frobnicate",
        "error: unrecognized subcommand 'frobnicate'",
    )], inspects 4 => super::repos::quoted_subcommand_fails,
    /// A repo without `CLAUDE.md` fails at line 1, with the fix that writes the import.
    missing_claude: Fails[fails(
        "CLAUDE.md:1",
        "CLAUDE.md does not exist",
        Fix::CreateImport {
            file: "CLAUDE.md".to_owned(),
            target: "AGENTS.md".to_owned(),
        },
    )], inspects 2 => super::repos::missing_claude,
    /// A repo without `ARCHITECTURE.md` fails at line 1.
    missing_architecture: Fails[fails(
        "ARCHITECTURE.md:1",
        "ARCHITECTURE.md does not exist",
        Fix::CreateRequired {
            file: "ARCHITECTURE.md".to_owned(),
        },
    )], inspects 2 => super::repos::missing_architecture,
    /// `CLAUDE.md` that imports another file is not `@AGENTS.md`.
    claude_wrong_import: Fails[fails(
        "CLAUDE.md:1",
        "CLAUDE.md must hold only `@AGENTS.md`",
        Fix::MoveImport {
            file: "CLAUDE.md".to_owned(),
            target: "AGENTS.md".to_owned(),
        },
    )], inspects 3 => super::repos::claude_wrong_import,
    /// A percent-escaped link resolves to the file with the space.
    percent_escaped_link_passes: Passes, inspects 5 => super::repos::percent_escaped_link_passes,
    /// Absolute URLs, pure anchors, `mailto:` and an empty destination are not looked up.
    urls_and_anchors_pass: Passes, inspects 4 => super::repos::urls_and_anchors_pass,
    /// A Markdown file git ignores is not one of the repo's files: it is not read or counted.
    ignored_markdown_is_not_read: Passes,
        inspects 3 => super::repos::ignored_markdown_is_not_read,
}
