//! Unit tests of the cfg rule's walk, for forms the planted cases do not show.

use super::*;

/// The findings of `source`, each as its kind and line.
fn findings(source: &str) -> Vec<(Finding, u32)> {
    let mut hits = Vec::new();
    walk(
        TokenStream::from_str(source).expect("the source lexes"),
        Scope::outside(),
        &mut hits,
    );
    hits
}

/// Forms the planted cases do not show: other delimiters, a metavariable in place of the
/// group, a nested `not`, and predicates that are malformed.
#[test]
fn forms_beyond_the_planted_cases() {
    let name = |name: &str| Finding::Predicate(format!("the name `{name}`"));
    let malformed = || Finding::Predicate("a form that is not a predicate".to_owned());
    let negated = || Finding::Predicate("a feature under `not`".to_owned());
    let cases = [
        ("cfg!{unix}; if cfg!(not(clippy)) {}", vec![]),
        (
            "macro_rules! cfg_select { () => {} }",
            vec![(Finding::CfgSelect, 1)],
        ),
        ("#[cfg $p]", vec![(metavariable(), 1)]),
        ("#[cfg_attr $p]", vec![(metavariable(), 1)]),
        ("#[cfg_attr($p, derive(Debug))]", vec![(metavariable(), 1)]),
        ("#![$a(x)]", vec![(Finding::MetavariableAttribute, 1)]),
        (
            "#[cfg_attr(test, $a)]",
            vec![(Finding::MetavariableAttribute, 1)],
        ),
        ("#[cfg(not(not(feature = \"a\")))]", vec![(negated(), 1)]),
        (
            "#[cfg(not(all(test, feature = \"a\")))]",
            vec![(negated(), 1)],
        ),
        ("#[cfg(feature)]", vec![(malformed(), 1)]),
        ("#[cfg(test, unix)]", vec![(malformed(), 1)]),
        ("#[cfg(not(test, test))]", vec![(malformed(), 1)]),
        ("#[cfg()]", vec![(malformed(), 1)]),
        (
            "#[cfg_attr(test, cfg_attr(unix, derive(Debug)))]",
            vec![(name("unix"), 1)],
        ),
        (
            "#[cfg_attr(test, cfg_attr(feature = \"a\", cfg(test)))]",
            vec![(Finding::CfgInCfgAttr, 1)],
        ),
        ("r#include!(\"x.rs\");", vec![(Finding::Include, 1)]),
        (
            "#[cfg(test,)] #[cfg(any())] #[cfg(all(test,))] fn f() {}",
            vec![],
        ),
        ("let cfg = 1; include_bytes!(\"x\"); path::x();", vec![]),
        ("self.cfg(); fn cfg(x: u8) {} cfg_attr(unix);", vec![]),
        (
            "let a = [path, cfg]; vec![$x]; m!([path = \"x\"]);",
            vec![(Finding::PathAttribute, 1)],
        ),
        ("#!$a", vec![(Finding::MetavariableAttribute, 1)]),
        ("cfg $b (unix)", vec![]),
        (
            "$($m)*!(x); $crate::m!(); $i::x!();",
            vec![(Finding::MetavariableMacro, 1)],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(findings(source), expected, "{source}");
    }
}

/// How the metavariable rules read the arms of a `macro_rules!`: matchers are skipped, and a
/// transcriber's `#[$m]` passes only when its own arm binds `$m` as `#[$m:meta]`.
#[test]
fn macro_arms() {
    let attribute = |line| (Finding::MetavariableAttribute, line);
    let cases = [
        ("macro_rules! m { ($i:ident $x:tt) => {}; }", vec![]),
        ("macro_rules! m { (#$h:tt $i:ident!) => {} }", vec![]),
        ("macro_rules! m { (#[path $e:tt]) => {} }", vec![]),
        ("macro_rules! m { (#[$a:meta]) => { #[$a] } }", vec![]),
        (
            "macro_rules! m { (#[$a:meta]) => {}; (#[$b:meta]) => { #[$a] } }",
            vec![attribute(1)],
        ),
        (
            "macro_rules! m { (#[$a:meta]) => { #![$a] } }",
            vec![attribute(1)],
        ),
        (
            "macro_rules! m { (#[$a:meta]) => { #[$a(x)] } }",
            vec![attribute(1)],
        ),
        (
            "macro_rules! m { ($a:meta) => { #[$a] } }",
            vec![attribute(1)],
        ),
        (
            "macro_rules! m { (#[$a:meta]) => { macro_rules! n { () => { #[$a] } } } }",
            vec![attribute(1)],
        ),
        (
            "macro_rules! m { (#[$a:meta]) => { #[cfg_attr(test, $a)] } }",
            vec![attribute(1)],
        ),
        (
            "macro_rules! m { (#[$a:meta]) => { $a!() } }",
            vec![(Finding::MetavariableMacro, 1)],
        ),
        (
            "macro_rules! m { (#[$a:meta]) => { #[$a] } garbage }",
            vec![attribute(1), attribute(1)],
        ),
        (
            "macro_rules! m { (#[cfg(unix)]) => {} }",
            vec![(Finding::Predicate("the name `unix`".to_owned()), 1)],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(findings(source), expected, "{source}");
    }
}
