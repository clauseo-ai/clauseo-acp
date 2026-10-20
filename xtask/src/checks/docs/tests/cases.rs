//! The planted repos of the docs check. Each builds a scratch git repo with its files, which the
//! check lists with `git ls-files` as it does in the real repo. A required file a case does not
//! write is valid, so each case shows only the problem it plants.

use crate::checks::planted::{Repo, git};
use crate::scratch::Scratch;
use std::fmt::Write;
use std::path::Path;

mod commands;
mod paths;
pub(super) use commands::*;
pub(super) use paths::*;

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
/// and the three required files, valid unless `files` gives one. `omit` names a required file
/// that is not written at all. `local` files are written but ignored (`.git/info/exclude`), so
/// git does not list them.
fn build(case: &str, files: Vec<(&str, String)>, omit: Option<&str>, local: &[&str]) -> Repo {
    let dir = Scratch::new(&format!("docs-{case}"));
    git(&dir, &["init", "-q"]);
    let mut all = vec![
        ("AGENTS.md", AGENTS.to_owned()),
        ("ARCHITECTURE.md", ARCHITECTURE.to_owned()),
        ("CLAUDE.md", CLAUDE.to_owned()),
    ];
    all.retain(|(name, _)| !files.iter().any(|(other, _)| other == name));
    all.extend(files);
    for (name, text) in all {
        if Some(name) != omit {
            write(&dir, name, &text);
        }
    }
    for name in local {
        write(&dir, name, "# Local\n");
    }
    let exclude = local
        .iter()
        .map(|name| format!("/{name}\n"))
        .collect::<Vec<_>>()
        .concat();
    write(&dir, ".git/info/exclude", &exclude);
    Repo::new(dir)
}

/// Writes `text` to `dir/name`, making its folder first.
fn write(dir: &Path, name: &str, text: &str) {
    if let Some(parent) = Path::new(name).parent() {
        Scratch::create_dir_all(dir.join(parent)).expect("create the folder");
    }
    Scratch::write(dir.join(name), text).expect("write a doc");
}

/// `build` with every required file written.
fn docs(case: &str, files: Vec<(&str, String)>) -> Repo {
    build(case, files, None, &[])
}

/// `lines` lines of `"line N"`, each ending in a newline.
fn lines(count: usize) -> String {
    let mut text = String::new();
    for n in 1..=count {
        writeln!(text, "line {n}").expect("write to a String");
    }
    text
}

pub(super) fn missing_backticked_path() -> Repo {
    docs(
        "missing-backticked-path",
        vec![(
            "GUIDE.md",
            "Start at `crates/nope/src/lib.rs`.\n".to_owned(),
        )],
    )
}

pub(super) fn broken_relative_link() -> Repo {
    docs(
        "broken-relative-link",
        vec![("GUIDE.md", "See [the notes](notes.md).\n".to_owned())],
    )
}

pub(super) fn missing_file_with_anchor() -> Repo {
    docs(
        "missing-file-with-anchor",
        vec![("GUIDE.md", "See [the notes](nope.md#x).\n".to_owned())],
    )
}

pub(super) fn broken_reference_definition() -> Repo {
    docs(
        "broken-reference-definition",
        vec![("GUIDE.md", "See [x][1].\n\n[1]: gone.md\n".to_owned())],
    )
}

pub(super) fn link_leaves_the_repo() -> Repo {
    docs(
        "link-leaves-the-repo",
        vec![("GUIDE.md", "Out: [x](../../outside.md)\n".to_owned())],
    )
}

pub(super) fn path_in_html_comment() -> Repo {
    docs(
        "path-in-html-comment",
        vec![("GUIDE.md", "<!-- keep `crates/gone/` out -->\n".to_owned())],
    )
}

pub(super) fn unplanned_crate_path() -> Repo {
    docs(
        "unplanned-crate-path",
        vec![("GUIDE.md", "See `crates/store/src/lib.rs`.\n".to_owned())],
    )
}

pub(super) fn agents_121_lines() -> Repo {
    docs("agents-121-lines", vec![("AGENTS.md", lines(121))])
}

pub(super) fn agents_8193_bytes() -> Repo {
    docs(
        "agents-8193-bytes",
        vec![("AGENTS.md", format!("{}\n", "a".repeat(8192)))],
    )
}

pub(super) fn architecture_301_lines() -> Repo {
    docs(
        "architecture-301-lines",
        vec![("ARCHITECTURE.md", format!("# Architecture\n{}", lines(300)))],
    )
}

pub(super) fn claude_extra_line() -> Repo {
    docs(
        "claude-extra-line",
        vec![("CLAUDE.md", "@AGENTS.md\nSee the rules too.\n".to_owned())],
    )
}

pub(super) fn missing_agents() -> Repo {
    build("missing-agents", Vec::new(), Some("AGENTS.md"), &[])
}

pub(super) fn clean_docs_pass() -> Repo {
    let guide = "# Guide\n\n\
        See [the architecture](ARCHITECTURE.md), [the notes](docs/notes.md#intro) and [self](#guide).\n\
        An [outside link](https://example.com/guide) and [mail](mailto:team@example.com).\n\
        A [matter link](../clauseo-acp-private/START-HERE.md) is not checked.\n\n\
        The planned crate: `crates/protocol/src/lib.rs` and `crates/protocol/`.\n\n\
        The real paths: `xtask/src/main.rs`, `docs/notes.md`, `.github/`.\n\n\
        A [query](docs/notes.md?plain=1) and [a climb back in](docs/../xtask/src/main.rs).\n\n\
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

pub(super) fn agents_at_120_lines_and_8192_bytes_passes() -> Repo {
    let mut text = String::new();
    for index in 0..120 {
        let width = if index < 32 { 68 } else { 67 };
        writeln!(text, "{}", "a".repeat(width)).expect("write to a String");
    }
    docs("agents-at-limits", vec![("AGENTS.md", text)])
}

pub(super) fn claude_without_newline_passes() -> Repo {
    docs(
        "claude-without-newline",
        vec![("CLAUDE.md", "@AGENTS.md".to_owned())],
    )
}

pub(super) fn html_link_to_a_missing_file_fails() -> Repo {
    docs(
        "html-link",
        vec![("GUIDE.md", "<a href=\"nope.md\">x</a>\n".to_owned())],
    )
}

pub(super) fn html_image_to_a_missing_file_fails() -> Repo {
    docs(
        "html-image",
        vec![("GUIDE.md", "<img alt=\"x\" src='nope.png'>\n".to_owned())],
    )
}

pub(super) fn uppercase_md_extension_is_checked() -> Repo {
    docs(
        "uppercase-md",
        vec![("GUIDE.MD", "See [x](nope.md).\n".to_owned())],
    )
}

pub(super) fn markdown_extension_is_checked() -> Repo {
    docs(
        "markdown-extension",
        vec![("guide.markdown", "See [x](nope.md).\n".to_owned())],
    )
}

/// `GUIDE.md` holding `text`, beside the file `xtask/src/main.rs`.
fn guide(case: &str, text: &str) -> Repo {
    docs(
        case,
        vec![
            ("GUIDE.md", text.to_owned()),
            ("xtask/src/main.rs", "fn main() {}\n".to_owned()),
        ],
    )
}

pub(super) fn query_is_cut_before_the_lookup() -> Repo {
    let text = "Raw: [real](docs/real.md?plain=1), [line](docs/real.md?plain=1#L2).\n\
        Gone: [nope](nope.md?plain=1).\n";
    docs(
        "query-cut",
        vec![
            ("GUIDE.md", text.to_owned()),
            ("docs/real.md", "# Real\n".to_owned()),
        ],
    )
}

pub(super) fn dot_dot_path_and_link_fail() -> Repo {
    guide("dot-dot", "See `crates/../../x` and [x](crates/../../x).\n")
}

pub(super) fn case_mismatch_fails() -> Repo {
    docs(
        "case-mismatch",
        vec![
            (
                "GUIDE.md",
                "See [x](docs/REAL.md) and `docs/REAL.md`.\n".to_owned(),
            ),
            ("docs/real.md", "# Real\n".to_owned()),
        ],
    )
}

pub(super) fn unlisted_local_file_fails() -> Repo {
    let files = vec![(
        "GUIDE.md",
        "See [x](local.md) and `docs/local.md`.\n".to_owned(),
    )];
    build(
        "unlisted-local",
        files,
        None,
        &["local.md", "docs/local.md"],
    )
}

pub(super) fn repeats_on_one_line_are_reported_once() -> Repo {
    guide(
        "repeats",
        "See [a](nope.md), [b](gone.md) and [c](nope.md).\n",
    )
}

pub(super) fn path_in_inline_html_comment() -> Repo {
    guide(
        "inline-comment",
        "Text <!-- keep `crates/gone/` out --> more.\n",
    )
}

pub(super) fn path_in_multiline_html_comment() -> Repo {
    guide(
        "multiline-comment",
        "<!--\nA note.\n`crates/gone/` is gone.\n-->\n",
    )
}

pub(super) fn path_with_angle_brackets_passes() -> Repo {
    guide("path-angles", "Crates live in `crates/<name>/`.\n")
}

pub(super) fn path_with_an_open_angle_passes() -> Repo {
    guide("path-open-angle", "Not a path: `crates/<name`.\n")
}

pub(super) fn path_with_a_close_angle_passes() -> Repo {
    guide("path-close-angle", "Not a path: `crates/name>`.\n")
}

pub(super) fn path_with_a_star_passes() -> Repo {
    guide("path-star", "Globs: `xtask/**` and `docs/*.md`.\n")
}

pub(super) fn path_with_a_question_mark_passes() -> Repo {
    guide("path-question", "A wildcard: `docs/000?.md`.\n")
}

pub(super) fn path_with_a_brace_passes() -> Repo {
    guide("path-brace", "Packages: `ui/{app,components}`.\n")
}

pub(super) fn path_with_an_ellipsis_passes() -> Repo {
    guide("path-ellipsis", "Any crate: `crates/...`.\n")
}

pub(super) fn missing_path_with_a_line_fails() -> Repo {
    guide("missing-path-line", "See `crates/gone.rs:12`.\n")
}

pub(super) fn fence_in_quote_in_list_item_hides_its_contents() -> Repo {
    guide(
        "fence-quote-item",
        "- item\n\n  > ```\n  > [x](nope.md) `crates/gone/`\n  > ```\n",
    )
}

/// A repo with no Markdown file at all, required ones included: the check looks and finds none.
pub(super) fn no_markdown_and_no_required_files() -> Repo {
    let dir = Scratch::new("docs-no-markdown");
    git(&dir, &["init", "-q"]);
    Repo::new(dir)
}

pub(super) fn path_with_an_anchor_passes() -> Repo {
    guide("path-anchor", "See `xtask/src/main.rs#sec`.\n")
}

pub(super) fn path_with_trailing_punctuation_passes() -> Repo {
    guide(
        "path-punctuation",
        "See `xtask/src/main.rs,` `xtask/src/main.rs.` `xtask/src/main.rs;` `xtask/src/main.rs:`.\n",
    )
}

pub(super) fn wrong_case_top_folder_fails() -> Repo {
    guide("wrong-case-top-folder", "See `Xtask/src/main.rs`.\n")
}

pub(super) fn unmarked_missing_path_fails() -> Repo {
    guide("unmarked-missing", "Packages: `ui/components` and more.\n")
}

pub(super) fn trailing_dot_dot_fails() -> Repo {
    guide("trailing-dot-dot", "Up: `crates/a/..` and `crates/..`.\n")
}
