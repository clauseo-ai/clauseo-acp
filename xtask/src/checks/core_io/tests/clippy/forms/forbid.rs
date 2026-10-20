//! The forms the forbid line must refuse, built into a crate of their own: each turns a ban off
//! in its own file, and clippy must fail it with E0453 or the ban's own error there. They stop
//! the build before clippy's lints run, so they cannot share a crate with the other forms.

use super::{Form, fail};

pub(super) fn forms() -> Vec<Form> {
    let read = "pub fn f() {\n    let _ = std::fs::read(\"x\");\n}\n";
    vec![
        fail(
            "forbid_inner_allow",
            &format!("#![allow(clippy::disallowed_methods, reason = \"x\")]\n{read}"),
        ),
        fail(
            "forbid_cfg_attr_allow",
            &format!("#![cfg_attr(all(), allow(clippy::disallowed_methods))]\n{read}"),
        ),
        fail(
            "forbid_outer_allow",
            &format!("#[allow(clippy::disallowed_methods, reason = \"x\")]\n{read}"),
        ),
        fail(
            "forbid_outer_allow_of_a_type",
            "#[allow(clippy::disallowed_types, reason = \"x\")]\n\
             pub fn f(_: std::fs::File) {}\n",
        ),
        fail(
            "forbid_macro_built_allow",
            "macro_rules! m {\n    ($a:ident) => {\n        #[$a(clippy::disallowed_methods)]\n        \
             pub fn f() {\n            let _ = std::fs::read(\"x\");\n        }\n    };\n}\nm!(allow);\n",
        ),
        fail(
            "forbid_expect",
            &format!("#[expect(clippy::disallowed_methods, reason = \"a fixture\")]\n{read}"),
        ),
    ]
}
