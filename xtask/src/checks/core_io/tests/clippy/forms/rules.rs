//! The old syn check's `macro_rules!` cases, each invoked: clippy sees a rule only where it
//! expands. Its case of a rule body that expands to match arms is gone: no invocation can stand
//! where match arms do, so that rule never expands.

use super::{Form, fail, pass};

/// The cases that must fail.
pub(super) fn failing() -> Vec<Form> {
    let read = "pub fn f() {\n    let _ = m!(std fs);\n}\n";
    vec![
        fail(
            "metavariable_module",
            "macro_rules! m {\n    ($x:ident) => {\n        std::$x::read(\"a\")\n    };\n}\n\
             pub fn f() {\n    let _ = m!(fs);\n}\n",
        ),
        fail(
            "metavariable_now",
            "use std::time::Instant;\nmacro_rules! m {\n    ($t:ident) => {\n        $t::now()\n    };\n}\n\
             pub fn f() {\n    let _ = m!(Instant);\n}\n",
        ),
        fail(
            "metavariable_qualified_now",
            "macro_rules! m {\n    ($t:ty) => {\n        <$t>::now()\n    };\n}\n\
             pub fn f() {\n    let _ = m!(std::time::Instant);\n}\n",
        ),
        fail(
            "metavariable_type_binding",
            "macro_rules! m {\n    ($t:ty) => {\n        type A = $t;\n        \
             pub fn f() -> A {\n            A::now()\n        }\n    };\n}\nm!(std::time::Instant);\n",
        ),
        fail(
            "metavariable_use_binding",
            "macro_rules! m {\n    ($p:path) => {\n        use $p as I;\n        pub fn f() {\n            \
             let _ = I::fs::read(\"a\");\n        }\n    };\n}\nm!(std);\n",
        ),
        fail(
            "metavariable_glob",
            "macro_rules! m {\n    ($x:ident) => {\n        use std::$x::*;\n    };\n}\nm!(fs);\n\
             pub fn f() {\n    let _ = read(\"a\");\n}\n",
        ),
        fail(
            "macro_rule_self_now",
            "pub trait N {\n    fn n() -> Self;\n}\nmacro_rules! mk {\n    ($t:ty) => {\n        \
             impl N for $t {\n            fn n() -> Self {\n                Self::now()\n            \
             }\n        }\n    };\n}\nmk!(std::time::Instant);\n",
        ),
        fail(
            "macro_rule_visibility",
            "macro_rules! m {\n    ($vis:vis) => {\n        $vis fn f() {\n            \
             use std::{fs};\n            let _ = fs::read(\"a\");\n        }\n    };\n}\nm!(pub);\n",
        ),
        fail(
            "macro_rule_repetition",
            "macro_rules! m {\n    ($($x:expr),*) => {{\n        use std::{fs};\n        \
             $( let _ = fs::read($x); )*\n    }};\n}\npub fn f() {\n    m!(\"a\", \"b\");\n}\n",
        ),
        fail(
            "rule_block_use_group",
            "macro_rules! m {\n    ($b:block) => {\n        fn f() $b\n        fn g() {\n            \
             use std::{fs};\n            let _ = fs::read(\"a\");\n        }\n    };\n}\nm!({});\n",
        ),
        fail(
            "rule_block_std_alias",
            "macro_rules! m {\n    ($b:block) => {\n        fn f() $b\n        fn g() {\n            \
             use std as s;\n            let _ = s::fs::read(\"a\");\n        }\n    };\n}\nm!({});\n",
        ),
        fail(
            "rule_lifetime",
            "macro_rules! m {\n    ($l:lifetime) => {\n        pub fn g<$l>(x: &$l str) {\n            \
             use std::{fs};\n            let _ = fs::read(x);\n        }\n    };\n}\nm!('a);\n",
        ),
        fail(
            "rule_repetition_builds_a_path",
            &format!(
                "macro_rules! m {{\n    ($($p:ident)*) => {{\n        $($p)::*::read(\"a\")\n    }};\n}}\n{read}"
            ),
        ),
        fail(
            "rule_leading_colon_repetition",
            &format!(
                "macro_rules! m {{\n    ($($p:ident)*) => {{\n        $(::$p)*::read(\"a\")\n    }};\n}}\n{read}"
            ),
        ),
    ]
}

/// The cases that must pass.
pub(super) fn passing() -> Vec<Form> {
    vec![
        pass(
            "macro_rule_newtype_deref",
            "use std::ops::Deref;\nmacro_rules! newtype {\n    ($name:ident, $inner:ty) => {\n        \
             pub struct $name($inner);\n        impl Deref for $name {\n            \
             type Target = $inner;\n            fn deref(&self) -> &Self::Target {\n                \
             &self.0\n            }\n        }\n    };\n}\nnewtype!(SessionId, String);\n",
        ),
        pass(
            "macro_rule_ops_two_types",
            "use std::ops::Add;\npub struct W(pub u64);\nmacro_rules! ops {\n    ($($t:ty),+) => {$(\n        \
             impl Add<$t> for W {\n            type Output = $t;\n            \
             fn add(self, rhs: $t) -> $t {\n                rhs\n            }\n        }\n    )+};\n}\n\
             ops!(u32, u64);\n",
        ),
        pass(
            "metavariable_alias_unused",
            "macro_rules! m {\n    ($t:ty) => {\n        pub type A = $t;\n    };\n}\nm!(std::time::Instant);\n",
        ),
        pass(
            "macro_rule_module_named_by_a_metavariable",
            "macro_rules! m {\n    ($m:ident) => {\n        pub mod $m {\n            pub fn now() {}\n            \
             pub fn g() {\n                now();\n            }\n        }\n    };\n}\nm!(clock);\n",
        ),
        pass(
            "rule_defines_its_own_now",
            "macro_rules! ts {\n    ($n:ident) => {\n        pub struct $n(u64);\n        impl $n {\n            \
             pub fn now() -> Self {\n                $n(0)\n            }\n            \
             pub fn later() -> Self {\n                Self::now()\n            }\n        }\n    };\n}\n\
             ts!(Stamp);\n",
        ),
        pass(
            "rule_patterns",
            "macro_rules! some_ref {\n    ($x:ident) => {\n        Some(ref $x)\n    };\n}\n\
             macro_rules! small {\n    ($x:ident) => {\n        $x @ 1..=5\n    };\n}\n\
             pub fn f(o: Option<u8>) -> u8 {\n    if let some_ref!(v) = o {\n        \
             return *v;\n    }\n    match 3u8 {\n        small!(n) => n,\n        _ => 0,\n    }\n}\n",
        ),
    ]
}
