//! The planted cases of the docs check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module.

crate::checks::planted::cases! {
    /// A backticked repo path that does not exist fails at the span's line.
    missing_backticked_path: FailsWith(&[
        "GUIDE.md:1: `crates/nope/src/lib.rs` does not exist\n    \
         fix: create `crates/nope/src/lib.rs`, or correct or remove the reference",
    ]) => super::cases::missing_backticked_path,
    /// A relative link to a missing file fails, naming the file it resolves to.
    broken_relative_link: FailsWith(&[
        "GUIDE.md:1: the relative link `notes.md` does not resolve: `notes.md` does not exist\n    \
         fix: create `notes.md`, or correct or remove the link",
    ]) => super::cases::broken_relative_link,
    /// The anchor is cut before the path is resolved, so `nope.md#x` fails on `nope.md`.
    missing_file_with_anchor: FailsWith(&[
        "GUIDE.md:1: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::missing_file_with_anchor,
    /// A reference definition is a link too, and fails at its own line.
    broken_reference_definition: FailsWith(&[
        "GUIDE.md:3: the relative link `gone.md` does not resolve: `gone.md` does not exist\n    \
         fix: create `gone.md`, or correct or remove the link",
    ]) => super::cases::broken_reference_definition,
    /// A relative link that climbs out of the repo, other than into the private folder, fails.
    link_leaves_the_repo: FailsWith(&[
        "GUIDE.md:1: the relative link `../../outside.md` leaves the repo\n    \
         fix: point it at a file inside the repo, or remove it",
    ]) => super::cases::link_leaves_the_repo,
    /// A code span inside an HTML comment counts like any other.
    path_in_html_comment: FailsWith(&[
        "GUIDE.md:1: `crates/gone/` does not exist\n    \
         fix: create `crates/gone/`, or correct or remove the reference",
    ]) => super::cases::path_in_html_comment,
    /// A span opened by two backticks counts like one opened by a single backtick.
    double_backtick_span: FailsWith(&[
        "GUIDE.md:1: `crates/gone/x.rs` does not exist\n    \
         fix: create `crates/gone/x.rs`, or correct or remove the reference",
    ]) => super::cases::double_backtick_span,
    /// A crate path whose row is not in the Crates table is not exempt.
    unplanned_crate_path: FailsWith(&[
        "GUIDE.md:1: `crates/store/src/lib.rs` does not exist\n    \
         fix: create `crates/store/src/lib.rs`, or correct or remove the reference",
    ]) => super::cases::unplanned_crate_path,
    /// An `xtask` subcommand that clap does not know fails at its line, with clap's first line.
    unknown_xtask_subcommand: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::unknown_xtask_subcommand,
    /// An unknown flag fails, as an unknown subcommand does.
    unknown_xtask_flag: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask check --frobnicate` does not parse: error: unexpected argument '--frobnicate' found\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::unknown_xtask_flag,
    /// A fenced line that starts with `cargo xtask` is checked the same way.
    unknown_xtask_command_in_fence: FailsWith(&[
        "GUIDE.md:2: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::unknown_xtask_command_in_fence,
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
    /// An indented code block is not a fence: the link after it is checked.
    indented_four_fence_is_code_not_a_fence: FailsWith(&[
        "GUIDE.md:4: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::indented_four_fence_is_code_not_a_fence,
    /// A fence indented up to three spaces opens, so its contents are not read.
    fence_indented_three_hides_its_contents: Passes => super::cases::fence_indented_three_hides_its_contents,
    /// A fence inside a list item, indented to the item's text, opens and hides its contents.
    fence_in_list_item_hides_its_contents: Passes => super::cases::fence_in_list_item_hides_its_contents,
    /// An `<a href>` to a missing file fails at the tag's line.
    html_link_to_a_missing_file_fails: FailsWith(&[
        "GUIDE.md:1: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::html_link_to_a_missing_file_fails,
    /// An `<img src>` to a missing file fails, with the quotes around it removed.
    html_image_to_a_missing_file_fails: FailsWith(&[
        "GUIDE.md:1: the relative link `nope.png` does not resolve: `nope.png` does not exist\n    \
         fix: create `nope.png`, or correct or remove the link",
    ]) => super::cases::html_image_to_a_missing_file_fails,
    /// A reference definition whose destination is on the next line fails at its label's line.
    definition_destination_on_next_line_fails: FailsWith(&[
        "GUIDE.md:3: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::definition_destination_on_next_line_fails,
    /// A reference definition inside a list item is read.
    definition_in_list_item_fails: FailsWith(&[
        "GUIDE.md:1: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::definition_in_list_item_fails,
    /// A reference definition inside a block quote is read.
    definition_in_block_quote_fails: FailsWith(&[
        "GUIDE.md:1: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::definition_in_block_quote_fails,
    /// A `$ ` prompt before `cargo xtask` in a fence does not hide the command.
    prompt_command_in_fence_fails: FailsWith(&[
        "GUIDE.md:2: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::prompt_command_in_fence_fails,
    /// `VAR=value` before `cargo xtask` in a span does not hide the command.
    env_assignment_command_in_span_fails: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::env_assignment_command_in_span_fails,
    /// A command after `&&` in a span is checked on its own.
    chained_command_in_span_fails: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::chained_command_in_span_fails,
    /// An indented code block line that starts a command is checked.
    indented_code_command_fails: FailsWith(&[
        "GUIDE.md:3: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::indented_code_command_fails,
    /// A fence that opens on a list-marker line is a fence, and its command is checked.
    list_marker_fence_command_fails: FailsWith(&[
        "GUIDE.md:2: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::list_marker_fence_command_fails,
    /// `--help` does not hide an unknown subcommand after it.
    help_with_an_unknown_subcommand_fails: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask --help frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::help_with_an_unknown_subcommand_fails,
    /// `--help` does not hide an unknown flag after it.
    help_with_an_unknown_flag_fails: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask check --help --frobnicate` does not parse: error: unexpected argument '--frobnicate' found\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::help_with_an_unknown_flag_fails,
    /// A `.MD` file is a Markdown file, and its links are checked.
    uppercase_md_extension_is_checked: FailsWith(&[
        "GUIDE.MD:1: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::uppercase_md_extension_is_checked,
    /// A `.markdown` file is a Markdown file, and its links are checked.
    markdown_extension_is_checked: FailsWith(&[
        "guide.markdown:1: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::markdown_extension_is_checked,
    /// A closing fence indented four spaces is text, so the fence it meant to close stays open.
    indented_closing_fence_does_not_close: FailsWith(&[
        "GUIDE.md:4: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::indented_closing_fence_does_not_close,
    /// An unclosed fence in a block quote ends with the quote.
    quoted_fence_ends_with_its_quote: FailsWith(&[
        "GUIDE.md:4: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::quoted_fence_ends_with_its_quote,
    /// An unclosed fence in a list item ends with the item.
    list_item_fence_ends_with_its_item: FailsWith(&[
        "GUIDE.md:4: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::list_item_fence_ends_with_its_item,
    /// A reference definition that starts a list item is read.
    definition_after_a_list_item_fails: FailsWith(&[
        "GUIDE.md:2: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::definition_after_a_list_item_fails,
    /// A block quote interrupts a paragraph, so a definition in it is read.
    definition_after_a_quote_line_fails: FailsWith(&[
        "GUIDE.md:2: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::definition_after_a_quote_line_fails,
    /// A setext underline ends the heading, so a definition after it is read.
    definition_after_a_setext_heading_fails: FailsWith(&[
        "GUIDE.md:3: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::definition_after_a_setext_heading_fails,
    /// A thematic break ends a paragraph, so a definition after it is read.
    definition_after_a_thematic_break_fails: FailsWith(&[
        "GUIDE.md:3: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => super::cases::definition_after_a_thematic_break_fails,
    /// A stderr redirection after a command is shell syntax, not an argument.
    stderr_redirect_passes: Passes => super::cases::stderr_redirect_passes,
    /// A stdout redirection after a command is shell syntax, not an argument.
    stdout_redirect_passes: Passes => super::cases::stdout_redirect_passes,
    /// A command in a subshell is read, and its closing parenthesis is not an argument.
    subshell_with_valid_command_passes: Passes => super::cases::subshell_with_valid_command_passes,
    /// A command in a subshell is checked, and fails on its unknown subcommand.
    subshell_with_unknown_subcommand_fails: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::subshell_with_unknown_subcommand_fails,
    /// A command in a command substitution is checked.
    command_substitution_fails: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => super::cases::command_substitution_fails,
    /// A `time` and `env` before the command are skipped, and the command is checked.
    time_and_env_prefix_fails: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
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
    /// Escaped brackets are text, not links, and a fence hides its contents.
    escaped_brackets_and_fences_pass: Passes => super::cases::escaped_brackets_and_fences_pass,
}
