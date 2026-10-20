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

/// A `path` value resolves against its file's folder, one component at a time, and may never
/// leave the crate's folder on the way.
#[test]
fn path_values_resolve_inside_the_crate() {
    let file = "crates/p/tests/events.rs";
    let cases = [
        (
            "support/fixtures.rs",
            Some("crates/p/tests/support/fixtures.rs"),
        ),
        ("./a//b.rs", Some("crates/p/tests/a/b.rs")),
        ("../src/lib.rs", Some("crates/p/src/lib.rs")),
        ("../../p/src/lib.rs", None),
        ("../../q/src/lib.rs", None),
        ("/etc/x.rs", None),
        ("a\\b.rs", None),
        ("C:x.rs", None),
        ("", None),
    ];
    for (value, expected) in cases {
        assert_eq!(
            paths::resolve(value, file, "crates/p").as_deref(),
            expected,
            "{value}"
        );
    }
}

/// Where a literal `path` passes: on a top-level `mod name;`, after other attributes and a
/// visibility, naming a file the crate lexes.
#[test]
fn path_attributes_pass_only_on_a_top_level_module_declaration() {
    let lexed: BTreeSet<&str> = ["crates/p/src/lib.rs", "crates/p/src/a.rs"].into();
    let top = paths::TopLevel {
        file: "crates/p/src/lib.rs",
        dir: "crates/p",
        lexed: &lexed,
    };
    let findings_at_top = |source: &str| {
        let mut hits = Vec::new();
        let scope = Scope {
            top: Some(&top),
            ..Scope::outside()
        };
        walk(
            TokenStream::from_str(source).expect("the source lexes"),
            scope,
            &mut hits,
        );
        hits
    };
    let path = |line| vec![(Finding::PathAttribute, line)];
    let cases = [
        ("#[path = \"a.rs\"] mod x;", vec![]),
        ("#[path = r\"a.rs\"] #[cfg(test)] pub(crate) mod x;", vec![]),
        ("#[path = \"b.rs\"] mod x;", path(1)),
        ("#[path = b\"a.rs\"] mod x;", path(1)),
        ("#![path = \"a.rs\"]", path(1)),
        ("#[path = \"a.rs\"] fn x() {}", path(1)),
        ("#[path = \"a.rs\"] mod x {}", path(1)),
        ("fn f() { #[path = \"a.rs\"] mod x; }", path(1)),
        ("m! { #[path = \"a.rs\"] mod x; }", path(1)),
    ];
    for (source, expected) in cases {
        assert_eq!(findings_at_top(source), expected, "{source}");
    }
}
