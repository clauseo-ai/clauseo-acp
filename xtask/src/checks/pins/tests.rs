//! Tests of the pins check; the planted cases are in `tests/planted.rs`, their repos in
//! `tests/cases.rs`.

mod cases;
mod engines_node;
mod fake_pins;
mod forms_text;
pub(super) mod planted;
mod workflow_files;
mod yaml_text;

use super::*;
use cases::{check, put, repo};

/// saphyr-parser's markers as `yaml.rs` reads them: lines from 1, a block scalar from the line
/// after its `|`, and columns in characters, so a `#` after a non-ASCII key is still placed.
#[test]
fn yaml_lines_and_comments_come_from_the_parser() {
    let text = "é: x # one\nk: \"q # not\" # two\nr: | # three\n  # text\n  more\n# four\n";
    let doc = yaml::parse(text).expect("valid YAML");
    let comments: Vec<_> = doc.comments(text);
    assert_eq!(
        comments,
        [(1, " one"), (2, " two"), (3, " three"), (6, " four")]
    );
    let [root] = doc.roots() else {
        panic!("one document");
    };
    let entries = root.entries();
    let block = entries
        .iter()
        .find(|(key, _)| key.scalar() == Some("r"))
        .map(|(_, value)| value)
        .expect("the block scalar");
    assert_eq!((block.line, block.end_line), (4, 6));
    assert_eq!(block.scalar(), Some("# text\nmore\n"));
    assert_eq!(
        yaml::parse("a: [\n").err(),
        Some(yaml::NotYaml {
            line: 2,
            info: "while parsing a node, did not find expected node content".to_owned(),
        })
    );
}

#[test]
fn a_comment_names_the_version_after_the_word_lefthook() {
    assert_eq!(
        lefthook::versions_named(" lefthook 9.8.7: `root` runs"),
        ["9.8.7"]
    );
    assert_eq!(lefthook::versions_named(" as of Lefthook v10.0."), ["10.0"]);
    assert_eq!(
        lefthook::versions_named(" lefthook@1.2.3, lefthook 4"),
        ["1.2.3", "4"]
    );
    let none: [&str; 0] = [];
    assert_eq!(lefthook::versions_named(" lefthook runs the hooks"), none);
    assert_eq!(lefthook::versions_named(" lefthook-local 1.2.3"), none);
    assert_eq!(lefthook::versions_named(" mylefthook 1.2.3"), none);
}

/// The real `lefthook.yml` names lefthook's version in a comment, so the rule compares a copy
/// on this repo, not none.
#[test]
fn the_real_lefthook_comment_names_the_pinned_version() {
    let root = crate::workspace::workspace_root();
    let text = std::fs::read_to_string(root.join(LEFTHOOK)).expect("read lefthook.yml");
    let doc = yaml::parse(&text).expect("lefthook.yml is YAML");
    let named: Vec<&str> = doc
        .comments(&text)
        .into_iter()
        .flat_map(|(_, comment)| lefthook::versions_named(comment))
        .collect();
    let pinned = crate::setup::BINARIES
        .iter()
        .find(|binary| binary.name == "lefthook")
        .map(|binary| binary.version);
    assert!(
        !named.is_empty(),
        "no comment in lefthook.yml names a version"
    );
    assert!(named.iter().all(|version| Some(*version) == pinned));
}

#[test]
fn a_version_counts_only_as_a_whole_word() {
    assert!(workflows::holds("npm i -g pnpm@7.8.9", "7.8.9"));
    assert!(workflows::holds("v7.8.9.", "7.8.9"));
    assert!(workflows::holds("rust:3.4", "3.4"));
    assert!(!workflows::holds("17.8.9", "7.8.9"));
    assert!(!workflows::holds("7.8.90", "7.8.9"));
    assert!(!workflows::holds("7.8.9.1", "7.8.9"));
    assert!(!workflows::holds("0.7.8.9", "7.8.9"));
    assert!(!workflows::holds("3.4.5", "3.4"));
}

#[test]
fn versions_are_two_or_three_numbers() {
    assert!(is_version("1.2"));
    assert!(is_version("1.2.3"));
    assert!(!is_version("1"));
    assert!(!is_version("1.2.3.4"));
    assert!(!is_version("1..3"));
    assert!(!is_version("stable"));
    assert_eq!(major_minor("1.2.3"), "1.2");
    assert_eq!(major_minor("1.2"), "1.2");
}

/// Each way a home can fail to give its value says why, with that value's fix.
#[test]
fn a_home_that_cannot_be_read_says_why() {
    let cases: [(&str, &str, &str, &str); 5] = [
        (
            "toolchain-not-toml",
            TOOLCHAIN,
            "[toolchain\n",
            "could not read the Rust version from rust-toolchain.toml: it is not TOML: unclosed \
             table, expected `]`",
        ),
        (
            "toolchain-no-channel",
            TOOLCHAIN,
            "[toolchain]\nprofile = \"minimal\"\n",
            "could not read the Rust version from rust-toolchain.toml: it has no `channel` string \
             under `[toolchain]`",
        ),
        (
            "package-no-manager",
            PACKAGE_JSON,
            "{ \"engines\": { \"node\": \">=6.7.0\" } }",
            "could not read the pnpm version from ui/package.json: it has no `packageManager` \
             string",
        ),
        (
            "package-node-range",
            PACKAGE_JSON,
            "{ \"engines\": { \"node\": \"*\" }, \"packageManager\": \"pnpm@7.8.9\" }",
            "could not read the minimum Node from ui/package.json: `engines.node` is `*`; \
             engines.node takes only \">=<major>.<minor>.<patch>\"",
        ),
        (
            "package-not-json",
            PACKAGE_JSON,
            "{ \"packageManager\": \"pnpm@7.8.9\" ",
            "could not read the pnpm version from ui/package.json: it is not JSON: EOF while \
             parsing an object at line 1 column 33",
        ),
    ];
    for (case, file, text, message) in cases {
        let dir = repo(case);
        put(&dir, file, text);
        let found = check(&dir);
        let messages: Vec<&str> = found
            .failures
            .iter()
            .map(|failure| failure.message.as_str())
            .collect();
        assert!(messages.contains(&message), "{case}: {messages:?}");
        assert!(
            found.violations.is_empty(),
            "{case}: {:?}",
            found.violations
        );
    }
}

/// `dtolnay/rust-toolchain` is pinned by a commit only when its ref is 40 hex digits. A shorter
/// hex ref, or a 40-character name that is not hex, is no commit, so its comment is not read.
#[test]
fn a_ref_is_a_commit_only_when_it_is_forty_hex_digits() {
    for reference in ["abcdef".to_owned(), "z".repeat(40)] {
        let dir = repo("dtolnay-ref");
        let text = format!(
            "jobs:\n  a:\n    steps:\n      - uses: dtolnay/rust-toolchain@{reference} # 1.85.0\n"
        );
        put(&dir, ".github/workflows/ci.yml", &text);
        let found = check(&dir);
        assert_eq!(
            (found.violations.is_empty(), found.failures.is_empty()),
            (true, true),
            "{reference}: {:?} {:?}",
            found.violations,
            found.failures
        );
    }
}
