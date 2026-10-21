//! Tests of the pins check; the planted cases are in `tests/planted.rs`, their repos in
//! `tests/repos.rs`.

mod engines_node;
mod fake_pins;
pub(super) mod planted;
mod repos;
mod yaml_text;

use super::*;
use repos::{check, put, repo};

/// saphyr-parser's markers as `yaml.rs` reads them: lines from 1, a block scalar from the line
/// after its `|`, and columns in characters, so a `#` after a non-ASCII key is still placed.
/// A parse error is at the line the parser stopped on.
#[test]
fn yaml_lines_and_comments_come_from_the_parser() {
    let text = "é: x # one\nk: \"q # not\" # two\nr: | # three\n  # text\n  more\n# four\n";
    let doc = yaml::parse(text).expect("valid YAML");
    let comments: Vec<_> = doc.comments(text);
    assert_eq!(
        comments,
        [(1, " one"), (2, " two"), (3, " three"), (6, " four")]
    );
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
