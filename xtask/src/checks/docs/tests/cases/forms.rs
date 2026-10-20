//! Planted repos for the forms a link, a command or a required file can take that the other
//! case files do not hold: links from the root, escaped, private or as images; commands after a
//! prompt, a toolchain, cargo flags, another command, or with a bad flag or value; a required file
//! missing or wrong on its own; and a Markdown file git ignores.

use super::{build, docs, guide};
use crate::checks::planted::Repo;

pub(in crate::checks::docs::tests) fn root_slash_link_fails() -> Repo {
    guide("root-slash-link", "See [x](/nope.md).\n")
}

pub(in crate::checks::docs::tests) fn percent_escaped_link_passes() -> Repo {
    docs(
        "percent-escaped-link",
        vec![
            ("GUIDE.md", "See [x](docs/my%20file.md).\n".to_owned()),
            ("docs/my file.md", "# Mine\n".to_owned()),
        ],
    )
}

/// The private repo beside this one is not checked: outside contributors do not have it.
pub(in crate::checks::docs::tests) fn private_folder_link_passes() -> Repo {
    guide(
        "private-folder-link",
        "Plans: [start](../clauseo-acp-private/START-HERE.md).\n",
    )
}

/// A link that names the private folder and then climbs out of it leaves the repo.
pub(in crate::checks::docs::tests) fn private_folder_then_climb_fails() -> Repo {
    guide(
        "private-then-climb",
        "Out: [x](../clauseo-acp-private/../outside.md).\n",
    )
}

pub(in crate::checks::docs::tests) fn urls_and_anchors_pass() -> Repo {
    guide(
        "urls-and-anchors",
        "[a](https://example.com/nope.md), [b](#top), [c](mailto:a@example.com), [d]().\n",
    )
}

pub(in crate::checks::docs::tests) fn markdown_image_to_a_missing_file_fails() -> Repo {
    guide("markdown-image", "![logo](nope.png)\n")
}

pub(in crate::checks::docs::tests) fn command_after_a_prompt_and_assignment_fails() -> Repo {
    guide(
        "command-after-prompt",
        "```sh\n$ RUST_LOG=1 cargo xtask frobnicate\n```\n",
    )
}

pub(in crate::checks::docs::tests) fn command_with_a_toolchain_fails() -> Repo {
    guide(
        "command-toolchain",
        "Run `cargo +nightly xtask frobnicate`.\n",
    )
}

pub(in crate::checks::docs::tests) fn command_after_cargo_flags_fails() -> Repo {
    guide("command-cargo-flags", "Run `cargo -q xtask frobnicate`.\n")
}

pub(in crate::checks::docs::tests) fn second_command_on_a_line_fails() -> Repo {
    guide(
        "second-command",
        "Run `cargo xtask check && cargo xtask frobnicate`.\n",
    )
}

pub(in crate::checks::docs::tests) fn unknown_flag_fails() -> Repo {
    guide("unknown-flag", "Run `cargo xtask check --frobnicate`.\n")
}

pub(in crate::checks::docs::tests) fn value_outside_its_range_fails() -> Repo {
    guide("value-range", "Run `cargo xtask buildtime --runs 0`.\n")
}

pub(in crate::checks::docs::tests) fn unknown_subcommand_after_help_fails() -> Repo {
    guide("after-help", "Run `cargo xtask --help frobnicate`.\n")
}

pub(in crate::checks::docs::tests) fn quoted_subcommand_fails() -> Repo {
    guide(
        "quoted-subcommand",
        "```sh\ncargo xtask 'frobnicate'\n```\n",
    )
}

pub(in crate::checks::docs::tests) fn missing_claude() -> Repo {
    build("missing-claude", Vec::new(), Some("CLAUDE.md"), &[])
}

pub(in crate::checks::docs::tests) fn missing_architecture() -> Repo {
    build(
        "missing-architecture",
        Vec::new(),
        Some("ARCHITECTURE.md"),
        &[],
    )
}

pub(in crate::checks::docs::tests) fn claude_wrong_import() -> Repo {
    docs(
        "claude-wrong-import",
        vec![("CLAUDE.md", "@README.md\n".to_owned())],
    )
}

/// A Markdown file git ignores is not one of the repo's files, so its broken link is not read.
pub(in crate::checks::docs::tests) fn ignored_markdown_is_not_read() -> Repo {
    let files = vec![("NOTES.md", "See [x](nope.md).\n".to_owned())];
    build("ignored-markdown", files, None, &["NOTES.md"])
}
