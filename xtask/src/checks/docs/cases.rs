//! The planted inputs of the docs check. Each builds a scratch repo with its files and runs
//! `inspect` on the Markdown files it wrote. A required file a case does not write is valid, so
//! each case shows only the problem it plants.

use super::inspect;
use crate::checks::Inspection;
use crate::scratch::Scratch;
use std::fmt::Write;
use std::fs;
use std::path::Path;

/// A valid `AGENTS.md`.
const AGENTS: &str = "# Agents\n\nRules for coding agents.\n";

/// A valid `ARCHITECTURE.md`: a Crates table with one planned row, `protocol`.
const ARCHITECTURE: &str = "# Architecture\n\n## Crates\n\n\
    | Crate | Owns | May depend on | Never depends on | Status |\n\
    |---|---|---|---|---|\n\
    | `xtask` | checks | none of ours | tauri | exists |\n\
    | `protocol` | events | serde | tauri | planned |\n";

/// A valid `CLAUDE.md`.
const CLAUDE: &str = "@AGENTS.md\n";

/// Builds a scratch repo named after `case` with `files` (each a repo-relative path and its text)
/// and the three required files, valid unless `files` gives one, then inspects the `.md` files.
/// `omit` names a required file that is not written at all.
fn build(case: &str, files: Vec<(&str, String)>, omit: Option<&str>) -> Inspection {
    let dir = Scratch::new(&format!("docs-{case}"));
    let mut all = vec![
        ("AGENTS.md", AGENTS.to_owned()),
        ("ARCHITECTURE.md", ARCHITECTURE.to_owned()),
        ("CLAUDE.md", CLAUDE.to_owned()),
    ];
    all.retain(|(name, _)| !files.iter().any(|(other, _)| other == name));
    all.extend(files);
    let mut names = Vec::new();
    for (name, text) in all {
        if Some(name) == omit {
            continue;
        }
        write(&dir, name, &text);
        names.push(name.to_owned());
    }
    names.sort();
    inspect(&dir, &names)
}

/// Writes `text` to `dir/name`, making its folder first.
fn write(dir: &Path, name: &str, text: &str) {
    if let Some(parent) = Path::new(name).parent() {
        fs::create_dir_all(dir.join(parent)).expect("create the folder");
    }
    fs::write(dir.join(name), text).expect("write a doc");
}

/// `build` with every required file written.
fn docs(case: &str, files: Vec<(&str, String)>) -> Inspection {
    build(case, files, None)
}

/// `lines` lines of `"line N"`, each ending in a newline.
fn lines(count: usize) -> String {
    let mut text = String::new();
    for n in 1..=count {
        writeln!(text, "line {n}").expect("write to a String");
    }
    text
}

pub(super) fn missing_backticked_path() -> Inspection {
    docs(
        "missing-backticked-path",
        vec![(
            "GUIDE.md",
            "Start at `crates/nope/src/lib.rs`.\n".to_owned(),
        )],
    )
}

pub(super) fn broken_relative_link() -> Inspection {
    docs(
        "broken-relative-link",
        vec![("GUIDE.md", "See [the notes](notes.md).\n".to_owned())],
    )
}

pub(super) fn missing_file_with_anchor() -> Inspection {
    docs(
        "missing-file-with-anchor",
        vec![("GUIDE.md", "See [the notes](nope.md#x).\n".to_owned())],
    )
}

pub(super) fn broken_reference_definition() -> Inspection {
    docs(
        "broken-reference-definition",
        vec![("GUIDE.md", "See [x][1].\n\n[1]: gone.md\n".to_owned())],
    )
}

pub(super) fn link_leaves_the_repo() -> Inspection {
    docs(
        "link-leaves-the-repo",
        vec![("GUIDE.md", "Out: [x](../../outside.md)\n".to_owned())],
    )
}

pub(super) fn path_in_html_comment() -> Inspection {
    docs(
        "path-in-html-comment",
        vec![("GUIDE.md", "<!-- keep `crates/gone/` out -->\n".to_owned())],
    )
}

pub(super) fn double_backtick_span() -> Inspection {
    docs(
        "double-backtick-span",
        vec![("GUIDE.md", "Use ``crates/gone/x.rs`` here.\n".to_owned())],
    )
}

pub(super) fn unplanned_crate_path() -> Inspection {
    docs(
        "unplanned-crate-path",
        vec![("GUIDE.md", "See `crates/store/src/lib.rs`.\n".to_owned())],
    )
}

pub(super) fn unknown_xtask_subcommand() -> Inspection {
    docs(
        "unknown-xtask-subcommand",
        vec![("GUIDE.md", "Run `cargo xtask frobnicate` now.\n".to_owned())],
    )
}

pub(super) fn unknown_xtask_flag() -> Inspection {
    docs(
        "unknown-xtask-flag",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask check --frobnicate`.\n".to_owned(),
        )],
    )
}

pub(super) fn unknown_xtask_command_in_fence() -> Inspection {
    docs(
        "unknown-xtask-command-in-fence",
        vec![(
            "GUIDE.md",
            "```sh\ncargo xtask frobnicate\n```\n".to_owned(),
        )],
    )
}

pub(super) fn agents_121_lines() -> Inspection {
    docs("agents-121-lines", vec![("AGENTS.md", lines(121))])
}

pub(super) fn agents_8193_bytes() -> Inspection {
    docs(
        "agents-8193-bytes",
        vec![("AGENTS.md", format!("{}\n", "a".repeat(8192)))],
    )
}

pub(super) fn architecture_301_lines() -> Inspection {
    docs(
        "architecture-301-lines",
        vec![("ARCHITECTURE.md", format!("# Architecture\n{}", lines(300)))],
    )
}

pub(super) fn claude_extra_line() -> Inspection {
    docs(
        "claude-extra-line",
        vec![("CLAUDE.md", "@AGENTS.md\nSee the rules too.\n".to_owned())],
    )
}

pub(super) fn missing_agents() -> Inspection {
    build("missing-agents", Vec::new(), Some("AGENTS.md"))
}

pub(super) fn clean_docs_pass() -> Inspection {
    let guide = "# Guide\n\n\
        See [the architecture](ARCHITECTURE.md), [the notes](docs/notes.md#intro) and [self](#guide).\n\
        An [outside link](https://example.com/guide) and [mail](mailto:team@example.com).\n\
        A [matter link](../clauseo-acp-private/START-HERE.md) is not checked.\n\n\
        The planned crate: `crates/protocol/src/lib.rs` and `crates/protocol/`.\n\n\
        The real paths: `xtask/src/main.rs`, `docs/notes.md`, `.github/`.\n\n\
        Run `cargo xtask check --quick`, `cargo xtask check --help`, or\n\
        `cargo xtask buildtime`.\n\n\
        <!-- a comment with `xtask/` in it -->\n\n\
        ```sh\n\
        cargo xtask check --quick  # changed crates only\n\
        ```\n\n\
        ~~~text\n\
        `crates/gone/nope.rs` is in a fenced block, so it is not checked.\n\
        ~~~\n";
    let notes = "# Notes\n\n## Intro\n\nSee [the guide](../GUIDE.md).\n";
    let files = vec![
        ("GUIDE.md", guide.to_owned()),
        ("docs/notes.md", notes.to_owned()),
        ("xtask/src/main.rs", "fn main() {}\n".to_owned()),
        (".github/pull_request_template.md", "# PR\n".to_owned()),
    ];
    docs("clean-docs-pass", files)
}

pub(super) fn agents_at_120_lines_and_8192_bytes_passes() -> Inspection {
    let mut text = String::new();
    for index in 0..120 {
        let width = if index < 32 { 68 } else { 67 };
        writeln!(text, "{}", "a".repeat(width)).expect("write to a String");
    }
    docs("agents-at-limits", vec![("AGENTS.md", text)])
}

pub(super) fn claude_without_newline_passes() -> Inspection {
    docs(
        "claude-without-newline",
        vec![("CLAUDE.md", "@AGENTS.md".to_owned())],
    )
}

pub(super) fn escaped_brackets_and_fences_pass() -> Inspection {
    let guide = "Not a link: \\[text](gone.md) and [text\\](gone.md).\n\n\
        Not a span: \\`crates/gone/x` here.\n\n\
        ````md\n\
        ```\n\
        [in a fence](gone.md) `crates/gone/x`\n\
        ```\n\
        ````\n";
    docs("escaped-brackets", vec![("GUIDE.md", guide.to_owned())])
}

pub(super) fn indented_four_fence_is_code_not_a_fence() -> Inspection {
    docs(
        "indented-four-fence",
        vec![("GUIDE.md", "Text.\n\n    ```\n[x](nope.md)\n".to_owned())],
    )
}

pub(super) fn fence_indented_three_hides_its_contents() -> Inspection {
    docs(
        "fence-indented-three",
        vec![("GUIDE.md", "   ```\n[x](nope.md)\n   ```\n".to_owned())],
    )
}

pub(super) fn fence_in_list_item_hides_its_contents() -> Inspection {
    docs(
        "fence-in-list-item",
        vec![(
            "GUIDE.md",
            "- item\n\n  ```\n[x](nope.md)\n  ```\n".to_owned(),
        )],
    )
}

pub(super) fn html_link_to_a_missing_file_fails() -> Inspection {
    docs(
        "html-link",
        vec![("GUIDE.md", "<a href=\"nope.md\">x</a>\n".to_owned())],
    )
}

pub(super) fn html_image_to_a_missing_file_fails() -> Inspection {
    docs(
        "html-image",
        vec![("GUIDE.md", "<img alt=\"x\" src='nope.png'>\n".to_owned())],
    )
}

pub(super) fn definition_destination_on_next_line_fails() -> Inspection {
    docs(
        "definition-next-line",
        vec![("GUIDE.md", "See [g].\n\n[g]:\n  nope.md\n".to_owned())],
    )
}

pub(super) fn definition_in_list_item_fails() -> Inspection {
    docs(
        "definition-list-item",
        vec![("GUIDE.md", "- [g]: nope.md\n".to_owned())],
    )
}

pub(super) fn definition_in_block_quote_fails() -> Inspection {
    docs(
        "definition-block-quote",
        vec![("GUIDE.md", "> [g]: nope.md\n".to_owned())],
    )
}

pub(super) fn prompt_command_in_fence_fails() -> Inspection {
    docs(
        "prompt-command",
        vec![(
            "GUIDE.md",
            "```sh\n$ cargo xtask frobnicate\n```\n".to_owned(),
        )],
    )
}

pub(super) fn env_assignment_command_in_span_fails() -> Inspection {
    docs(
        "env-command",
        vec![(
            "GUIDE.md",
            "Run `RUST_LOG=1 cargo xtask frobnicate`.\n".to_owned(),
        )],
    )
}

pub(super) fn chained_command_in_span_fails() -> Inspection {
    docs(
        "chained-command",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask check && cargo xtask frobnicate`.\n".to_owned(),
        )],
    )
}

pub(super) fn indented_code_command_fails() -> Inspection {
    docs(
        "indented-code-command",
        vec![(
            "GUIDE.md",
            "Text.\n\n    cargo xtask frobnicate\n".to_owned(),
        )],
    )
}

pub(super) fn list_marker_fence_command_fails() -> Inspection {
    docs(
        "list-marker-fence",
        vec![(
            "GUIDE.md",
            "- ```sh\n  cargo xtask frobnicate\n  ```\n".to_owned(),
        )],
    )
}

pub(super) fn help_with_an_unknown_subcommand_fails() -> Inspection {
    docs(
        "help-unknown-subcommand",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask --help frobnicate`.\n".to_owned(),
        )],
    )
}

pub(super) fn help_with_an_unknown_flag_fails() -> Inspection {
    docs(
        "help-unknown-flag",
        vec![(
            "GUIDE.md",
            "Run `cargo xtask check --help --frobnicate`.\n".to_owned(),
        )],
    )
}

pub(super) fn uppercase_md_extension_is_checked() -> Inspection {
    docs(
        "uppercase-md",
        vec![("GUIDE.MD", "See [x](nope.md).\n".to_owned())],
    )
}

pub(super) fn markdown_extension_is_checked() -> Inspection {
    docs(
        "markdown-extension",
        vec![("guide.markdown", "See [x](nope.md).\n".to_owned())],
    )
}
