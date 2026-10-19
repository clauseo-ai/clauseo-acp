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

pub(crate) fn prompt_command_in_fence_fails() -> Inspection {
    docs(
        "prompt-command",
        vec![(
            "GUIDE.md",
            "```sh\n$ cargo xtask frobnicate\n```\n".to_owned(),
        )],
    )
}

pub(crate) fn env_assignment_command_in_span_fails() -> Inspection {
    docs(
        "env-command",
        vec![(
            "GUIDE.md",
            "Run `RUST_LOG=1 cargo xtask frobnicate`.\n".to_owned(),
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

pub(crate) fn stdout_redirect_passes() -> Inspection {
    docs(
        "stdout-redirect",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask check > out.txt`.\n".to_owned(),
        )],
    )
}

pub(crate) fn subshell_with_valid_command_passes() -> Inspection {
    docs(
        "subshell-valid",
        vec![(
            "GUIDE.md",
            "Run `(cd x && cargo xtask check)`.\n".to_owned(),
        )],
    )
}

pub(crate) fn subshell_with_unknown_subcommand_fails() -> Inspection {
    docs(
        "subshell-unknown",
        vec![(
            "GUIDE.md",
            "Run `(cd x && cargo xtask frobnicate)`.\n".to_owned(),
        )],
    )
}

pub(crate) fn command_substitution_fails() -> Inspection {
    docs(
        "command-substitution",
        vec![("GUIDE.md", "Run `$(cargo xtask frobnicate)`.\n".to_owned())],
    )
}

pub(crate) fn time_and_env_prefix_fails() -> Inspection {
    docs(
        "time-env-prefix",
        vec![(
            "GUIDE.md",
            "Run `time env cargo xtask frobnicate`.\n".to_owned(),
        )],
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
