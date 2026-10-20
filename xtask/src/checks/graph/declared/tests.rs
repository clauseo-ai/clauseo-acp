//! Tests of the text that says what is known about the blamed lines, and of the version match.
//! The rest of the matching of each declaration to its node runs in the tests of `dependencies`
//! and `plugins`.

use super::{Blamed, Group, produced, satisfies};
use crate::checks::graph::{Section, Tree};
use crate::workspace::CargoDependency;

/// A group of `lines` that is `known` or not.
fn group(lines: &[u32], known: bool) -> Group {
    Group {
        lines: lines.to_vec(),
        known,
    }
}

/// What `Blamed::declared` says for the keys `keys` and the groups `groups`, which bring `b`
/// in, in Cargo.toml.
fn declared(keys: &[&str], groups: &[Group]) -> String {
    let mut lines: Vec<u32> = groups.iter().flat_map(|g| g.lines.clone()).collect();
    lines.sort_unstable();
    let blamed = Blamed {
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        lines,
        optional: false,
        groups: groups.to_vec(),
    };
    blamed.declared("Cargo.toml", "b")
}

#[test]
fn declared_says_each_line_brings_the_package_in_only_when_each_node_has_one_line() {
    let say = |groups: &[Group]| declared(&["s"], groups);
    assert_eq!(
        say(&[group(&[7], true), group(&[10], true)]),
        "`s` is declared at lines 7 and 10 of Cargo.toml, and each declaration brings `b` in"
    );
    assert_eq!(
        say(&[group(&[7, 10], true)]),
        "`s` is declared at lines 7 and 10 of Cargo.toml. Cargo builds these declarations as one \
         package, and one or more of them brings `b` in"
    );
    assert_eq!(
        say(&[group(&[7, 10], true), group(&[13], true)]),
        "`s` is declared at lines 7, 10 and 13 of Cargo.toml. Cargo builds the declarations at \
         lines 7 and 10 as one package, and one or more of them brings `b` in. The declaration at \
         line 13 brings `b` in"
    );
}

#[test]
fn declared_says_lines_not_matched_to_one_node_may_bring_the_package_in() {
    let say = |groups: &[Group]| declared(&["s1", "s2"], groups);
    assert_eq!(
        say(&[group(&[7, 10], false)]),
        "`s1` and `s2` are declared at lines 7 and 10 of Cargo.toml. xtask could not tell which \
         package cargo builds for each of these declarations, so each of them may bring `b` in"
    );
    assert_eq!(
        say(&[group(&[7], true), group(&[10], false)]),
        "`s1` and `s2` are declared at lines 7 and 10 of Cargo.toml. The declaration at line 7 \
         brings `b` in. xtask could not tell which package cargo builds for the declaration at line \
         10, so it may bring `b` in"
    );
}

#[test]
fn a_requirement_picks_the_version_cargo_would() {
    assert_eq!(satisfies("^1", "1.4.0"), Some(true));
    assert_eq!(satisfies("^1", "2.0.0"), Some(false));
    assert_eq!(satisfies(">=1, <2", "2.0.0"), Some(false));
    assert_eq!(satisfies("^2", "not a version"), None);
}

/// `cargo metadata`'s entry for a registry dependency on `s` with the requirement `req`.
fn registry(req: &str) -> CargoDependency {
    let entry = format!(
        r#"{{"name": "s", "rename": null, "kind": null, "target": null, "optional": false,
            "features": [], "req": "{req}"}}"#
    );
    serde_json::from_str(&entry).expect("the entry parses")
}

/// The `semver` crate cannot read the version 2.0, so that package may satisfy any requirement.
/// Next to 1.4.0, which satisfies `^1`, it leaves a `^1` declaration matched to neither.
/// Next to 1.4.0, which does not satisfy `^2`, it is the one package a `^2` declaration fits.
#[test]
fn a_version_semver_cannot_read_counts_as_a_possible_match() {
    let stdout = "clauseo-protocol v0.1.0 []\n|-- s v1.4.0 []\n`-- s v2.0 []\n";
    let tree = Tree::parse(stdout, "clauseo-protocol").expect("the tree parses");
    let produced = |req: &str| produced(&tree, &registry(req), Section::Normal);
    assert_eq!(produced("^1"), (vec![1, 2], false));
    assert_eq!(produced("^2"), (vec![2], true));
}
