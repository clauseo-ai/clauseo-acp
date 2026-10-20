//! Unit tests of the cfg rule's walk, for forms the planted cases do not show.

use super::*;

/// The findings of `source`, each as its kind and line.
fn findings(source: &str) -> Vec<(Finding, u32)> {
    findings_of(TokenStream::from_str(source).expect("the source lexes"))
}

/// The findings of the lexed `tokens`, each as its kind and line.
fn findings_of(tokens: TokenStream) -> Vec<(Finding, u32)> {
    let mut hits = Vec::new();
    walk(
        tokens,
        Scope {
            definitions: true,
            ..Scope::outside()
        },
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
        ("#!$a", vec![(Finding::HashWithoutBrackets, 1)]),
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
        ("macro_rules! m { ($i:ident $x:expr) => {}; }", vec![]),
        ("macro_rules! m { ($h:ident $i:ident!) => {} }", vec![]),
        ("macro_rules! m { (#[path $e:expr]) => {} }", vec![]),
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
        (
            "macro_rules! m { ($v:ident $t:expr) => { $v[0]; $crate[0]; $t[0]; $u[0]; $($t),*[0] } }",
            vec![],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(findings(source), expected, "{source}");
    }
}

/// Every `#` is followed by a bracket group, or by `!` and a bracket group, in a matcher, a
/// transcriber and a call's arguments alike. Doc comments, raw identifiers and strings pass.
#[test]
fn every_hash_is_followed_by_brackets() {
    let hash = |line| (Finding::HashWithoutBrackets, line);
    let cases = [
        (
            "//! Docs.\n/// Docs.\n#[derive(Debug)] #![allow(dead_code)] fn r#f() { \"#\"; }",
            vec![],
        ),
        ("m!(#, [cfg(test)]);", vec![hash(1)]),
        ("m!(#);", vec![hash(1)]),
        ("m!(#!);", vec![hash(1)]),
        ("m!(#(test) #{test} #! !);", vec![hash(1), hash(1), hash(1)]),
        ("macro_rules! m { (#$h:ident) => {} }", vec![hash(1)]),
        ("macro_rules! m { () => { # x } }", vec![hash(1)]),
        (
            "macro_rules! m { ($($h:ident)?) => { $($h)? [test] } }",
            vec![],
        ),
        (
            "m! { macro_rules! n { (#[$a:meta]) => { #[$a] } } }",
            vec![(Finding::MetavariableAttribute, 1); 2],
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(findings(source), expected, "{source}");
    }
}

/// The name `tt` fails wherever it stands: a raw `r#tt`, a fragment kind in a nested definition
/// or in a call's arguments, and an identifier. A kind passed in as `$h:$k` fails at the call.
#[test]
fn the_name_tt_fails_everywhere() {
    let tt = |line| (Finding::TtFragment, line);
    let cases = [
        ("let tt = 1; let r#tt = 2;", vec![tt(1), tt(1)]),
        (
            "macro_rules! m { () => { macro_rules! n { ($h:tt) => {} } } }",
            vec![tt(1)],
        ),
        (
            "macro_rules! m { ($k:ident) => { m2!($h:$k); } }\nm!(tt);",
            vec![tt(2)],
        ),
        ("let ttt = 1; let t = 2; \"tt\";", vec![]),
    ];
    for (source, expected) in cases {
        assert_eq!(findings(source), expected, "{source}");
    }
}

/// A shebang first line is left out, and an inner attribute on the first line is kept.
#[test]
fn a_shebang_first_line_is_left_out() {
    let cases = [
        ("#!/usr/bin/env rust-script\nfn f() {}", true),
        ("#!/// Docs.\nfn f() {}", true),
        ("#!/usr/bin/env \"x\nfn f() {}", true),
        ("#!", true),
        ("#![forbid(clippy::disallowed_methods)]\nfn f() {}", false),
        ("#! /* a */ [allow(dead_code)]", false),
        ("#!\n[allow(dead_code)]", false),
        ("#!\n\"x", false),
        ("fn f() {}", false),
    ];
    for (text, shebang) in cases {
        let lexed = TokenStream::from_str(text);
        assert_eq!(is_shebang(text, &lexed), shebang, "{text}");
    }
    assert_eq!(
        blank_first_line("#!/usr/bin/env rust-script\nfn f() {}"),
        "\nfn f() {}"
    );
    let name = Finding::Predicate("the name `unix`".to_owned());
    let shebang = "#!/usr/bin/env rust-script\n#[cfg(unix)] fn f() {}";
    let lexed = lex(shebang).expect("the shebang lexes once it is blanked");
    assert_eq!(findings_of(lexed), vec![(name, 2)]);
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
            definitions: true,
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
