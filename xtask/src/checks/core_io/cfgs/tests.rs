//! Unit tests of the cfg rule's walk, for forms the planted cases do not show.

use super::*;

/// The findings of `source`, each as its kind and line.
fn findings(source: &str) -> Vec<(Finding, u32)> {
    let mut hits = Vec::new();
    walk(
        TokenStream::from_str(source).expect("the source lexes"),
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
