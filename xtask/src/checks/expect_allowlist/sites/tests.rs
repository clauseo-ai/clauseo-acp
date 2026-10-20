//! Tests of `find`: which attributes are sites, the item each names, and the line.

use super::*;

/// The sites of `text` as `<line> <item> <kind>`, where kind is `expect <lint>` or
/// `allow <lint>`, with `!` after the word for an inner attribute, or `allow()` for one with no
/// lint.
fn found(path: &str, text: &str) -> Vec<String> {
    let sites = find(path, text).expect("the snippet parses");
    sites
        .into_iter()
        .map(|site| {
            let kind = match site.kind {
                Kind::Expect { lint, inner } => format!("expect{} {lint}", bang(inner)),
                Kind::Allow { lint, inner } => format!("allow{} {lint}", bang(inner)),
                Kind::EmptyAllow => "allow()".to_owned(),
            };
            format!("{} {} {kind}", site.line, site.item)
        })
        .collect()
}

fn bang(inner: bool) -> &'static str {
    if inner { "!" } else { "" }
}

#[test]
fn an_attribute_on_a_named_item_names_it() {
    let text = "#[expect(a)]\nfn f() {}\n\
                #[expect(b)]\nconst C: u8 = 1;\n\
                #[expect(c)]\nstatic S: u8 = 1;\n\
                #[expect(d)]\nmod m {}\n\
                impl T {\n    #[expect(e)]\n    fn method() {}\n    #[expect(f)]\n    \
                const K: u8 = 1;\n}\n\
                #[expect(g)]\nfn r#type() {}\n";
    assert_eq!(
        found("src/a.rs", text),
        [
            "1 f expect a",
            "3 C expect b",
            "5 S expect c",
            "7 m expect d",
            "10 method expect e",
            "12 K expect f",
            "15 type expect g",
        ]
    );
}

#[test]
fn an_attribute_on_anything_else_names_the_item_around_it() {
    let text = "#[expect(a)]\nstruct S {\n    #[expect(b)]\n    x: u8,\n}\n\
                #[expect(c)]\nimpl S {}\n\
                trait T {\n    #[expect(d)]\n    fn t() {}\n}\n\
                fn f() {\n    #[expect(e)]\n    let x = 1;\n    let g = || {\n        \
                #[expect(h)]\n        fn inner() {}\n    };\n}\n\
                mod m {\n    #[expect(i)]\n    enum E {}\n}\n";
    assert_eq!(
        found("xtask/src/checks/core.rs", text),
        [
            "1 core expect a",
            "3 core expect b",
            "6 core expect c",
            "9 core expect d",
            "13 f expect e",
            "16 inner expect h",
            "21 m expect i",
        ]
    );
}

#[test]
fn a_file_s_module_is_its_stem_its_folder_for_mod_rs_and_crate_for_a_root() {
    assert_eq!(module_name("crates/a/src/lib.rs"), "crate");
    assert_eq!(module_name("xtask/src/main.rs"), "crate");
    assert_eq!(module_name("crates/desktop/build.rs"), "crate");
    assert_eq!(module_name("crates/a/src/os/mod.rs"), "os");
    assert_eq!(module_name("xtask/src/checks/budgets/tests.rs"), "tests");
    assert_eq!(module_name("xtask/tests/process.rs"), "process");
    assert_eq!(
        found("src/lib.rs", "#[expect(a)]\nstruct S;\n"),
        ["1 crate expect a"]
    );
}

#[test]
fn an_inner_attribute_names_the_file_top_or_the_item_it_is_in() {
    let text = "#![expect(a)]\nfn f() {\n    #![expect(b)]\n}\nmod m {\n    #![expect(c)]\n}\n";
    assert_eq!(
        found("src/a.rs", text),
        ["1 - expect! a", "3 f expect! b", "6 m expect! c"]
    );
}

#[test]
fn cfg_attr_counts_at_any_depth_after_its_predicate() {
    let text = "#![cfg_attr(all(), allow(d))]\n\
                #[cfg_attr(test, expect(a))]\nfn f() {}\n\
                #[cfg_attr(unix, cfg_attr(test, expect(b), allow(c)))]\nfn g() {}\n\
                #[cfg_attr(expect, derive(Debug))]\nstruct S;\n";
    assert_eq!(
        found("src/a.rs", text),
        [
            "1 - allow! d",
            "2 f expect a",
            "4 g expect b",
            "4 g allow c",
        ]
    );
}

#[test]
fn every_lint_of_one_attribute_is_a_site_and_a_reason_is_not() {
    let text = "#[expect(\n    a,\n    clippy :: b,\n    reason = \"c, d\"\n)]\nfn f() {}\n";
    assert_eq!(
        found("src/a.rs", text),
        ["1 f expect a", "1 f expect clippy::b"]
    );
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
        "        #[expect($lint)]\n",
        "        #[$meta]\n",
        "        /// #[expect(d)] in a doc\n",
        "        const S: &str = \"#[expect(e)]\";\n",
        "    };\n",
        "}\n",
        "fn f() {\n",
        "    other! { #[allow(f)] x }\n",
        "}\n",
    );
    assert_eq!(
        found("src/a.rs", text),
        [
            "3 a expect a",
            "5 a allow! b",
            "6 a expect c",
            "7 a expect $lint",
            "14 f allow f",
        ]
    );
}

#[test]
fn doc_comments_and_strings_that_mention_an_attribute_are_not_sites() {
    let text = "//! Uses `#[expect(a)]`.\n/// And #![allow(b)].\n#[doc = \"#[expect(c)]\"]\n\
                fn f() {\n    let s = \"#[expect(d)]\";\n    /* #[allow(e)] */\n}\n";
    assert_eq!(found("src/a.rs", text), Vec::<String>::new());
}

#[test]
fn a_raw_identifier_names_the_same_attribute() {
    let text = "#[r#allow(a)]\nfn f() {}\n#[cfg_attr(test, r#expect(b))]\nfn g() {}\n";
    assert_eq!(found("src/a.rs", text), ["1 f allow a", "3 g expect b"]);
}

#[test]
fn an_allow_that_names_no_lint_is_a_site() {
    let text = "#[allow()]\nfn f() {}\n#[allow]\nfn g() {}\n";
    assert_eq!(found("src/a.rs", text), ["1 f allow()", "3 g allow()"]);
}

#[test]
fn a_file_that_is_not_rust_is_an_error_at_its_line() {
    let error = find("src/a.rs", "fn f() {}\nfn g( {\n").expect_err("not Rust");
    assert_eq!(error.span().start().line, 2);
}

/// An attribute written inside another attribute's brackets is not a site: the outer one is
/// read as a whole, and neither holds a meta that names the inner one.
#[test]
fn an_attribute_inside_a_macro_attribute_is_not_searched_again() {
    let text = concat!(
        "macro_rules! m {\n",
        "    () => {\n",
        "        #[cfg_attr(test, #[expect(a, reason = \"x\")])]\n",
        "        #[doc = #[expect(b)]]\n",
        "        fn g() {}\n",
        "    };\n",
        "}\n",
    );
    assert_eq!(found("src/a.rs", text), Vec::<String>::new());
}
