//! The planted inputs of the docs check that hold `cargo xtask` commands.

use super::{docs, guide};
use crate::checks::Inspection;

pub(crate) fn unknown_xtask_subcommand() -> Inspection {
    docs(
        "unknown-xtask-subcommand",
        vec![("GUIDE.md", "Run `cargo xtask frobnicate` now.\n".to_owned())],
    )
}

pub(crate) fn unknown_xtask_flag() -> Inspection {
    docs(
        "unknown-xtask-flag",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask check --frobnicate`.\n".to_owned(),
        )],
    )
}

pub(crate) fn unknown_xtask_command_in_fence() -> Inspection {
    docs(
        "unknown-xtask-command-in-fence",
        vec![(
            "GUIDE.md",
            "```sh\ncargo xtask frobnicate\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn trailing_backslash_command_fails() -> Inspection {
    docs(
        "trailing-backslash",
        vec![(
            "GUIDE.md",
            "```sh\ncargo xtask frobnicate \\\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn prompt_command_in_fence_fails() -> Inspection {
    docs(
        "prompt-command",
        vec![(
            "GUIDE.md",
            "```sh\n$ cargo xtask frobnicate\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn chained_command_in_span_fails() -> Inspection {
    docs(
        "chained-command",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask check && cargo xtask frobnicate`.\n".to_owned(),
        )],
    )
}

pub(crate) fn indented_code_command_fails() -> Inspection {
    docs(
        "indented-code-command",
        vec![(
            "GUIDE.md",
            "Text.\n\n    cargo xtask frobnicate\n".to_owned(),
        )],
    )
}

pub(crate) fn help_with_an_unknown_subcommand_fails() -> Inspection {
    docs(
        "help-unknown-subcommand",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask --help frobnicate`.\n".to_owned(),
        )],
    )
}

pub(crate) fn help_with_an_unknown_flag_fails() -> Inspection {
    docs(
        "help-unknown-flag",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask check --help --frobnicate`.\n".to_owned(),
        )],
    )
}

pub(crate) fn stderr_redirect_passes() -> Inspection {
    docs(
        "stderr-redirect",
        vec![("GUIDE.md", "Run `cargo xtask check 2>&1`.\n".to_owned())],
    )
}

pub(crate) fn tab_indented_code_command_fails() -> Inspection {
    guide(
        "tab-indented",
        "Text.\n\n\tcargo xtask frobnicate\n\t[x](nope.md)\n",
    )
}

pub(crate) fn bare_tool_span_passes() -> Inspection {
    guide("bare-tool", "Run `cargo xtask` with a subcommand.\n")
}

pub(crate) fn bare_tool_in_code_fails() -> Inspection {
    guide("bare-tool-code", "```sh\ncargo xtask\n```\n")
}

pub(crate) fn placeholder_subcommand_passes() -> Inspection {
    guide(
        "placeholder-subcommand",
        "`cargo xtask <command> --help` lists its flags.\n",
    )
}

pub(crate) fn ellipsis_subcommand_passes() -> Inspection {
    guide(
        "ellipsis-subcommand",
        "Any `cargo xtask ...` here parses.\n",
    )
}

pub(crate) fn placeholder_flag_value_passes() -> Inspection {
    guide(
        "placeholder-flag-value",
        "Run `cargo xtask buildtime --runs <n>`.\n",
    )
}

pub(crate) fn unknown_subcommand_before_a_placeholder_fails() -> Inspection {
    guide(
        "placeholder-after-unknown",
        "Run `cargo xtask frobnicate <x>`.\n",
    )
}

pub(crate) fn toolchain_command_fails() -> Inspection {
    guide("toolchain", "Run `cargo +nightly xtask frobnicate`.\n")
}

pub(crate) fn quoted_subcommand_passes() -> Inspection {
    docs(
        "quoted-subcommand",
        vec![("GUIDE.md", "Run `cargo xtask \"check\"`.\n".to_owned())],
    )
}

/// A `text` and a `text,ignore` block that show a failing command and its error, as output.
pub(crate) fn text_block_output_passes() -> Inspection {
    docs(
        "text-block",
        vec![(
            "GUIDE.md",
            "```text\n$ cargo xtask frobnicate\nerror: unrecognized subcommand 'frobnicate'\n```\n\n\
             ```text,ignore\n$ cargo xtask frobnicate\n```\n"
                .to_owned(),
        )],
    )
}

pub(crate) fn apostrophe_in_plain_block_fails() -> Inspection {
    docs(
        "apostrophe-in-plain-block",
        vec![(
            "GUIDE.md",
            "```\nDon't run cargo xtask frobnicate\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn unclosed_quote_after_a_pipe_fails() -> Inspection {
    docs(
        "unclosed-quote-after-a-pipe",
        vec![(
            "GUIDE.md",
            "```sh\ncargo xtask frobnicate | grep \"it's\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn plain_block_wrapped_command_fails() -> Inspection {
    docs(
        "plain-block-wrapped",
        vec![(
            "GUIDE.md",
            "```\ncd x && cargo xtask frobnicate\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn plain_block_prose_fails() -> Inspection {
    docs(
        "plain-block-prose",
        vec![(
            "GUIDE.md",
            "```\nRun cargo xtask to see the commands.\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn background_command_passes() -> Inspection {
    docs(
        "background-command",
        vec![("GUIDE.md", "Run `cargo xtask check &`.\n".to_owned())],
    )
}

/// The reviewer's prose lines, each in its kind of block: a mention of `cargo xtask` in rust,
/// markdown and yaml blocks, and output in a console block, is not a command.
pub(crate) fn prose_in_code_blocks_passes() -> Inspection {
    docs(
        "prose-in-code-blocks",
        vec![(
            "GUIDE.md",
            "```rust\n// run cargo xtask check after editing\n\
             /// Checked by cargo xtask check on every push.\n```\n\n\
             ```console\n$ cargo xtask check\nerror: run cargo xtask setup first\n```\n\n\
             ```markdown\n- [ ] cargo xtask check passes\n```\n\n\
             ```md\nRun cargo xtask to see the commands.\n```\n\n\
             ```yaml\n- name: cargo xtask check (quick)\n```\n"
                .to_owned(),
        )],
    )
}

/// A `# ` comment, a `> ` quoted line and output in a console block are not commands.
pub(crate) fn output_and_comments_in_sessions_pass() -> Inspection {
    docs(
        "session-output",
        vec![(
            "GUIDE.md",
            "```console\n# then run cargo xtask check again\n> cargo xtask check passes now\n```\n\n\
             ```console\n$ cargo xtask check\nerror: run cargo xtask setup first\n```\n"
                .to_owned(),
        )],
    )
}
