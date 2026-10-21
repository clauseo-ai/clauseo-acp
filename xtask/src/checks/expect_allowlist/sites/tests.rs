//! Tests of `find`: which attributes and macros are sites, the item each names, and the line.

use super::*;

/// The sites of `text` as `<line> <item> <kind>`. The kind is `expect <lint>`, `allow <lint>` or
/// `warn <lint>`, with `!` after the word for an inner attribute; `allow()` or `warn()` for one
/// with no lint; `$` for a metavariable attribute; `#` for a `#` with no brackets; `path <value>`
/// (with `+` when it resolves) or `include <target>`, `?` for a value that is not a literal.
fn found(text: &str) -> Vec<String> {
    let sites = find(text).expect("the snippet parses");
    sites
        .into_iter()
        .map(|site| {
            let kind = match site.kind {
                Kind::Expect { lint, inner } => format!("expect{} {lint}", bang(inner)),
                Kind::Lower { level, lint, inner } => format!("{level}{} {lint}", bang(inner)),
                Kind::EmptyLower { level } => format!("{level}()"),
                Kind::Metavariable => "$".to_owned(),
                Kind::HashWithoutBrackets => "#".to_owned(),
                Kind::Path { value, resolve } => format!(
                    "path{} {}",
                    if resolve { "+" } else { "" },
                    value.as_deref().unwrap_or("?")
                ),
                Kind::Include { target } => {
                    format!("include {}", target.as_deref().unwrap_or("?"))
                }
            };
            format!("{} {} {kind}", site.line, site.item)
        })
        .collect()
}

fn bang(inner: bool) -> &'static str {
    if inner { "!" } else { "" }
}

#[test]
fn an_attribute_names_the_item_it_sits_on_as_a_path() {
    let text = concat!(
        "#[expect(a)]\nfn f() {}\n",
        "#[expect(b)]\nconst C: u8 = 1;\n",
        "#[expect(c)]\nstatic S: u8 = 1;\n",
        "struct Entry {\n    #[expect(d)]\n    replacement: u8,\n}\n",
        "impl fmt::Display for crate::A<T> {\n    #[expect(e)]\n    fn fmt() {}\n}\n",
        "impl fmt::Debug for A {\n    #[expect(f)]\n    fn fmt() {}\n    #[expect(g)]\n",
        "    const K: u8 = 1;\n}\n",
        "trait T {\n    #[expect(h)]\n    fn m() {}\n}\n",
        "mod m {\n    mod n {\n        #[expect(i)]\n        fn f() {}\n    }\n}\n",
        "enum E {\n    #[expect(j)]\n    V(#[expect(k)] u8),\n}\n",
        "#[expect(l)]\nfn r#type() {}\n",
        "#[expect(m)]\nimpl &[B] {}\n",
    );
    assert_eq!(
        found(text),
        [
            "1 f expect a",
            "3 C expect b",
            "5 S expect c",
            "8 Entry::replacement expect d",
            "12 A::fmt expect e",
            "16 A::fmt expect f",
            "18 A::K expect g",
            "22 T::m expect h",
            "27 m::n::f expect i",
            "32 E::V expect j",
            "33 E::V::0 expect k",
            "35 type expect l",
            "37 B expect m",
        ]
    );
}

#[test]
fn a_statement_or_expression_names_the_item_around_it() {
    let text = concat!(
        "fn f() {\n    #[expect(a)]\n    let x = 1;\n",
        "    let g = || {\n        #[expect(b)]\n        fn inner() {}\n    };\n}\n",
        "impl A {\n    fn m(#[expect(c)] x: u8) {\n        #[expect(d)]\n        call();\n    }\n}\n",
    );
    assert_eq!(
        found(text),
        [
            "2 f expect a",
            "5 f::inner expect b",
            "10 A::m expect c",
            "11 A::m expect d",
        ]
    );
}

#[test]
fn an_attribute_at_the_top_of_the_file_on_no_named_item_is_dash() {
    let text = "#![expect(a)]\n#[expect(b)]\nuse x::y;\n#[expect(c)]\nm! {}\n";
    assert_eq!(
        found(text),
        ["1 - expect! a", "2 - expect b", "4 - expect c"]
    );
}

#[test]
fn an_inner_attribute_names_the_item_it_is_in() {
    let text = "fn f() {\n    #![expect(b)]\n}\nmod m {\n    #![expect(c)]\n}\n";
    assert_eq!(found(text), ["2 f expect! b", "5 m expect! c"]);
}

#[test]
fn cfg_attr_counts_at_any_depth_after_its_predicate() {
    let text = "#![cfg_attr(all(), allow(d))]\n\
                #[cfg_attr(test, expect(a))]\nfn f() {}\n\
                #[cfg_attr(unix, cfg_attr(test, expect(b), allow(c)))]\nfn g() {}\n\
                #[cfg_attr(expect, derive(Debug))]\nstruct S;\n";
    assert_eq!(
        found(text),
        [
            "1 - allow! d",
            "2 f expect a",
            "4 g expect b",
            "4 g allow c",
        ]
    );
}

#[test]
fn warn_fails_as_allow_does_and_deny_and_forbid_pass() {
    let text = concat!(
        "#![warn(clippy::disallowed_methods)]\n",
        "#[warn(warnings, clippy::disallowed_methods)]\nfn f() {}\n",
        "#[cfg_attr(test, warn(dead_code))]\nfn g() {}\n",
        "#[warn()]\nfn h() {}\n",
        "#[deny(dead_code)]\n#[forbid(unsafe_code)]\nfn i() {}\n",
        "macro_rules! m {\n    () => {\n        #![warn(unused)]\n    };\n}\n",
    );
    assert_eq!(
        found(text),
        [
            "1 - warn! clippy::disallowed_methods",
            "2 f warn warnings",
            "2 f warn clippy::disallowed_methods",
            "4 g warn dead_code",
            "6 h warn()",
            "13 m warn! unused",
        ]
    );
}

#[test]
fn every_lint_of_one_attribute_is_a_site_and_a_reason_is_not() {
    let text = "#[expect(\n    a,\n    clippy :: b,\n    reason = \"c, d\"\n)]\nfn f() {}\n";
    assert_eq!(found(text), ["1 f expect a", "1 f expect clippy::b"]);
}

#[test]
fn macro_bodies_are_walked_for_attributes() {
    let text = concat!(
        "macro_rules! m {\n",
        "    () => {\n",
        "        #[expect(a)]\n",
        "        fn g() {}\n",
        "        #![allow(b)]\n",
        "        #[cfg_attr(test, expect(c))]\n",
        "        /// #[expect(d)] in a doc\n",
        "        const S: &str = \"#[expect(e)]\";\n",
        "    };\n",
        "}\n",
        "fn f() {\n",
        "    other! { #[allow(f)] x }\n",
        "}\n",
    );
    assert_eq!(
        found(text),
        [
            "3 m expect a",
            "5 m allow! b",
            "6 m expect c",
            "12 f allow f"
        ]
    );
}

/// The four shapes from review, the expect form and the outer forms: a caller could pass in
/// any attribute through each.
#[test]
fn an_attribute_whose_name_or_lint_is_a_metavariable_is_a_site() {
    let text = concat!(
        "macro_rules! q {\n",
        "    ($m:meta, $n:ident, $($t:tt)*) => {\n",
        "        #[$m]\n",
        "        #[$n(dead_code)]\n",
        "        #[cfg_attr(all(), $m)]\n",
        "        #![$($t)*]\n",
        "        #![$m]\n",
        "        #[expect($n, reason = \"x\")]\n",
        "        #[cfg_attr(test, allow(dead_code, $n))]\n",
        "        #[doc = $d]\n",
        "    };\n",
        "}\n",
    );
    assert_eq!(
        found(text),
        [
            "3 q $", "4 q $", "5 q $", "6 q $", "7 q $", "8 q $", "9 q $"
        ]
    );
}

#[test]
fn a_hash_in_a_macro_body_with_no_brackets_is_a_site() {
    let text = "macro_rules! q {\n    ($b:tt) => {\n        # $b\n        #! $b\n    };\n}\n";
    assert_eq!(found(text), ["3 q #", "4 q #"]);
}

#[test]
fn a_hash_before_parentheses_in_a_macro_body_is_not_an_attribute() {
    let text = "macro_rules! q {\n    () => {\n        #(expect(a))\n    };\n}\n";
    assert_eq!(found(text), ["3 q #"]);
}

#[test]
fn an_include_in_a_macro_body_does_not_hide_the_attribute_after_it() {
    let text = concat!(
        "macro_rules! i {\n    () => {\n",
        "        let a = 1; include!(\"c.rs\");\n",
        "        #[expect(b)]\n",
        "    };\n}\n",
    );
    assert_eq!(found(text), ["3 i include c.rs", "4 i expect b"]);
}

#[test]
fn doc_comments_and_strings_that_mention_an_attribute_are_not_sites() {
    let text = "//! Uses `#[expect(a)]`.\n/// And #![allow(b)].\n#[doc = \"#[expect(c)]\"]\n\
                fn f() {\n    let s = \"#[expect(d)]\";\n    /* #[allow(e)] */\n}\n";
    assert_eq!(found(text), Vec::<String>::new());
}

#[test]
fn a_raw_identifier_names_the_same_attribute() {
    let text = "#[r#allow(a)]\nfn f() {}\n#[cfg_attr(test, r#expect(b))]\nfn g() {}\n";
    assert_eq!(found(text), ["1 f allow a", "3 g expect b"]);
}

#[test]
fn an_allow_that_names_no_lint_is_a_site() {
    let text = "#[allow()]\nfn f() {}\n#[allow]\nfn g() {}\n#[allow = \"x\"]\nfn h() {}\n";
    assert_eq!(found(text), ["1 f allow()", "3 g allow()", "5 h allow()"]);
}

/// `#[path]` resolves only on a module at the top level of the file, outside macro bodies.
#[test]
fn path_attributes_are_sites() {
    let text = concat!(
        "#[path = \"x.txt\"]\nmod x;\n",
        "#[cfg_attr(unix, path = \"y.rs\")]\nmod y;\n",
        "mod outer {\n    #[path = \"z.rs\"]\n    mod z;\n}\n",
        "#[path = concat!(\"a\", \".rs\")]\nmod a;\n",
        "macro_rules! p {\n    () => {\n        #[path = \"b.rs\"]\n        mod b;\n    };\n}\n",
    );
    assert_eq!(
        found(text),
        [
            "1 x path+ x.txt",
            "3 y path+ y.rs",
            "6 outer::z path z.rs",
            "9 a path+ ?",
            "13 p path b.rs",
        ]
    );
}

#[test]
fn include_calls_are_sites_in_code_and_in_macro_bodies() {
    let text = concat!(
        "include!(\"a.inc\");\n",
        "fn f() {\n    std::include!(concat!(env!(\"OUT_DIR\"), \"/b.rs\"));\n}\n",
        "macro_rules! i {\n    () => {\n        include!(\"c.rs\");\n    };\n}\n",
        "const S: &str = include_str!(\"d.txt\");\n",
    );
    assert_eq!(
        found(text),
        ["1 - include a.inc", "3 f include ?", "7 i include c.rs"]
    );
}

#[test]
fn a_file_that_is_not_rust_is_an_error_at_its_line() {
    let error = find("fn f() {}\nfn g( {\n").expect_err("not Rust");
    assert_eq!(error.span().start().line, 2);
}
