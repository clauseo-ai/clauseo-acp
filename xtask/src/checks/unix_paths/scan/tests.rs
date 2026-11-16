//! Tests of the token walk: what covers a literal, and the cfg predicates.

use super::*;
use std::str::FromStr;

/// The literals of `text` that start with `/opt/`, as (line, unix, abs, test).
fn literals(text: &str) -> Vec<(u32, bool, bool, bool)> {
    scan(text, &["/opt/"])
        .expect("the text lexes")
        .literals
        .iter()
        .map(|found| (found.line, found.unix, found.abs, found.test))
        .collect()
}

#[test]
fn a_cfg_covers_its_item_and_stops_at_its_end() {
    let text = "#[cfg(unix)]\nconst A: S = S { p: \"/opt/a\" };\nconst B: &str = \"/opt/b\";\n\
                fn f() {\n    #[cfg(unix)]\n    { let a = \"/opt/c\"; }\n    let b = \"/opt/d\";\n}\n";
    assert_eq!(
        literals(text),
        [
            (2, true, false, false),
            (3, false, false, false),
            (6, true, false, false),
            (7, false, false, false),
        ]
    );
}

#[test]
fn a_cfg_on_a_match_arm_or_a_field_covers_only_that_arm_or_field() {
    let text = "fn f(x: u8) {\n    match x {\n        #[cfg(unix)]\n        0 => { \"/opt/a\"; }\n        \
                _ => { \"/opt/b\"; }\n    }\n    let s = S {\n        #[cfg(unix)]\n        a: \"/opt/c\",\n        \
                b: \"/opt/d\",\n    };\n}\n";
    assert_eq!(
        literals(text),
        [
            (4, true, false, false),
            (5, false, false, false),
            (9, true, false, false),
            (10, false, false, false),
        ]
    );
}

#[test]
fn cfg_test_marks_test_code_and_abs_marks_its_argument() {
    let text = "#[cfg(test)]\nmod tests {\n    fn f() { crate::testing::abs(\"/opt/a\"); x.abs(\"/opt/b\"); }\n}\n";
    assert_eq!(
        literals(text),
        [(3, false, true, true), (3, false, false, true)]
    );
}

#[test]
fn mod_lines_keep_their_path_inline_modules_and_cfg() {
    let text =
        "#[cfg(unix)]\n#[path = \"x/y.rs\"]\npub(crate) mod y;\nmod outer {\n    mod inner;\n}\n";
    let found = scan(text, &["/opt/"]).expect("the text lexes");
    assert_eq!(
        found.mods,
        [
            ModDecl {
                name: "y".to_owned(),
                path: Some("x/y.rs".to_owned()),
                inline: Vec::new(),
                unix: true,
            },
            ModDecl {
                name: "inner".to_owned(),
                path: None,
                inline: vec!["outer".to_owned()],
                unix: false,
            },
        ]
    );
}

/// The predicate inside `cfg(..)` in `attribute`.
fn predicate(attribute: &str) -> Predicate {
    let trees: Vec<TokenTree> = TokenStream::from_str(attribute)
        .expect("the predicate lexes")
        .into_iter()
        .collect();
    parse_group(trees.get(1))
}

#[test]
fn predicates_that_never_hold_on_windows() {
    let never = [
        "cfg(unix)",
        "cfg(all(test, unix))",
        "cfg(not(windows))",
        "cfg(target_os = \"macos\")",
        "cfg(target_os = \"linux\")",
        "cfg(target_family = \"unix\")",
        "cfg(any(target_os = \"macos\", target_os = \"linux\"))",
        "cfg(not(target_os = \"windows\"))",
        "cfg(not(any(windows, test)))",
    ];
    for text in never {
        assert!(predicate(text).never_on_windows(), "{text}");
    }
    let sometimes = [
        "cfg(windows)",
        "cfg(test)",
        "cfg(any(unix, windows))",
        "cfg(not(unix))",
        "cfg(feature = \"x\")",
        "cfg(any())",
        "cfg(not(all(windows, test)))",
    ];
    for text in sometimes {
        assert!(!predicate(text).never_on_windows(), "{text}");
    }
}

#[test]
fn predicates_that_hold_only_in_tests() {
    assert!(predicate("cfg(test)").requires_test());
    assert!(predicate("cfg(all(test, feature = \"x\"))").requires_test());
    assert!(!predicate("cfg(any(test, unix))").requires_test());
    assert!(!predicate("cfg(not(test))").requires_test());
}
