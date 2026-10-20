//! The planted inputs of the docs check that hold `cargo xtask` commands.

use super::{docs, guide};
use crate::checks::Inspection;

pub(crate) fn unknown_xtask_subcommand() -> Inspection {
    docs(
        "unknown-xtask-subcommand",
        vec![("GUIDE.md", "Run `cargo xtask frobnicate` now.\n".to_owned())],
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

pub(crate) fn tab_indented_code_command_fails() -> Inspection {
    guide(
        "tab-indented",
        "Text.\n\n\tcargo xtask frobnicate\n\t[x](nope.md)\n",
    )
}

pub(crate) fn continued_command_fails() -> Inspection {
    docs(
        "continued-command",
        vec![(
            "GUIDE.md",
            "```sh\ncargo xtask \\\nfrobnicate\n```\n".to_owned(),
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

pub(crate) fn plain_block_prose_fails() -> Inspection {
    docs(
        "plain-block-prose",
        vec![(
            "GUIDE.md",
            "```\nRun cargo xtask to see the commands.\n```\n".to_owned(),
        )],
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
