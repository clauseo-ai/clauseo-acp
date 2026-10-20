//! Tests of the text that says what is known about the blamed lines. The matching of each
//! declaration to its node runs in the tests of `dependencies` and `plugins`.

use super::{Group, declared, satisfies};

/// A group of `lines` that is `known` or not.
fn group(lines: &[u32], known: bool) -> Group {
    Group {
        lines: lines.to_vec(),
        known,
    }
}

#[test]
fn declared_says_each_line_brings_the_package_in_only_when_each_node_has_one_line() {
    let s = ["s".to_owned()];
    let say = |groups: &[Group]| declared(&s, "Cargo.toml", "b", groups);
    assert_eq!(
        say(&[group(&[7], true), group(&[10], true)]),
        "`s` is declared at lines 7 and 10 of Cargo.toml, and each declaration brings b in"
    );
    assert_eq!(
        say(&[group(&[7, 10], true)]),
        "`s` is declared at lines 7 and 10 of Cargo.toml. Cargo builds these declarations as one \
         package, and one or more of them brings b in"
    );
    assert_eq!(
        say(&[group(&[7, 10], true), group(&[13], true)]),
        "`s` is declared at lines 7, 10 and 13 of Cargo.toml. Cargo builds the declarations at \
         lines 7 and 10 as one package, and one or more of them brings b in. The declaration at \
         line 13 brings b in"
    );
}

#[test]
fn declared_says_lines_not_matched_to_one_node_may_bring_the_package_in() {
    let keys = ["s1".to_owned(), "s2".to_owned()];
    let say = |groups: &[Group]| declared(&keys, "Cargo.toml", "b", groups);
    assert_eq!(
        say(&[group(&[7, 10], false)]),
        "`s1` and `s2` are declared at lines 7 and 10 of Cargo.toml. xtask could not tell which \
         package cargo builds for each of these declarations, so each of them may bring b in"
    );
    assert_eq!(
        say(&[group(&[7], true), group(&[10], false)]),
        "`s1` and `s2` are declared at lines 7 and 10 of Cargo.toml. The declaration at line 7 \
         brings b in. xtask could not tell which package cargo builds for the declaration at line \
         10, so it may bring b in"
    );
}

#[test]
fn a_requirement_picks_the_version_cargo_would() {
    assert_eq!(satisfies("^1", "1.4.0"), Some(true));
    assert_eq!(satisfies("^1", "2.0.0"), Some(false));
    assert_eq!(satisfies(">=1, <2", "2.0.0"), Some(false));
    assert_eq!(satisfies("^2", "not a version"), None);
}
