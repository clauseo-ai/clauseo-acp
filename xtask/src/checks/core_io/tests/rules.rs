//! The core-io planted cases for `macro_rules!` bodies: metavariables, repetitions and rules
//! whose bodies core-io cannot parse.

use super::lib;
use crate::checks::Inspection;

/// A `macro_rules!` whose one rule expands to `expansion` (on line 3).
fn rule(case: &str, matcher: &str, expansion: &str) -> Inspection {
    lib(
        case,
        &format!("macro_rules! m {{\n    ({matcher}) => {{\n        {expansion}\n    }};\n}}\n"),
    )
}

pub(in crate::checks::core_io) fn metavariable_module() -> Inspection {
    rule("metavariable-module", "$x:ident", "std::$x::read(\"a\")")
}

pub(in crate::checks::core_io) fn metavariable_now() -> Inspection {
    rule("metavariable-now", "$t:ident", "$t::now()")
}

pub(in crate::checks::core_io) fn metavariable_qualified_now() -> Inspection {
    rule("metavariable-qualified-now", "$t:ty", "<$t>::now()")
}

pub(in crate::checks::core_io) fn metavariable_type_binding() -> Inspection {
    rule(
        "metavariable-type",
        "$t:ty",
        "type A = $t;\n        pub fn f() -> A {\n            A::now()\n        }",
    )
}

pub(in crate::checks::core_io) fn metavariable_use_binding() -> Inspection {
    rule(
        "metavariable-use",
        "$p:path",
        "use $p as I;\n        pub fn f() {\n            let _ = I::fs::read(\"a\");\n        }",
    )
}

pub(in crate::checks::core_io) fn macro_rule_use_group() -> Inspection {
    lib(
        "macro-rule-use-group",
        "macro_rules! m { ($x:expr) => {{ use std::{fs}; fs::read($x) }}; }\n",
    )
}

pub(in crate::checks::core_io) fn macro_rule_std_alias() -> Inspection {
    lib(
        "macro-rule-std-alias",
        "macro_rules! m { ($x:expr) => {{ use std as s; s::fs::read($x) }}; }\n",
    )
}

pub(in crate::checks::core_io) fn macro_rule_two_metavariables() -> Inspection {
    lib(
        "macro-rule-two-metavariables",
        "macro_rules! j { ($a:ident $b:ident) => { $a::$b::read(\"x\") }; }\n\n\
         pub fn f() {\n    let _ = j!(std fs);\n}\n",
    )
}

pub(in crate::checks::core_io) fn macro_rule_self_now() -> Inspection {
    lib(
        "macro-rule-self-now",
        "pub trait N {\n    fn n() -> Self;\n}\n\nmacro_rules! mk {\n    ($t:ty) => {\n        \
         impl N for $t {\n            fn n() -> Self {\n                Self::now()\n            \
         }\n        }\n    };\n}\n\nmk!(std::time::Instant);\n",
    )
}

pub(in crate::checks::core_io) fn macro_rule_newtype_deref() -> Inspection {
    lib(
        "macro-rule-newtype-deref",
        "use std::ops::Deref;\n\nmacro_rules! newtype {\n    ($name:ident, $inner:ty) => {\n        \
         pub struct $name($inner);\n\n        impl Deref for $name {\n            \
         type Target = $inner;\n\n            fn deref(&self) -> &Self::Target {\n                \
         &self.0\n            }\n        }\n    };\n}\n\nnewtype!(SessionId, String);\n",
    )
}

pub(in crate::checks::core_io) fn macro_rule_ops_two_types() -> Inspection {
    lib(
        "macro-rule-ops",
        "use std::ops::Add;\n\npub struct W(pub u64);\n\nmacro_rules! ops {\n    \
         ($($t:ty),+) => {$(\n        impl Add<$t> for W {\n            type Output = $t;\n\n            \
         fn add(self, rhs: $t) -> $t {\n                rhs\n            }\n        }\n    )+};\n}\n\n\
         ops!(u32, u64);\n",
    )
}

pub(in crate::checks::core_io) fn metavariable_alias_unused() -> Inspection {
    rule("metavariable-alias-unused", "$t:ty", "type A = $t;")
}

pub(in crate::checks::core_io) fn macro_rule_visibility() -> Inspection {
    lib(
        "macro-rule-visibility",
        "macro_rules! m {\n    ($vis:vis) => {\n        $vis fn f() {\n            \
         use std::{fs};\n            let _ = fs::read(\"a\");\n        }\n    };\n}\n",
    )
}

pub(in crate::checks::core_io) fn macro_rule_repetition() -> Inspection {
    lib(
        "macro-rule-repetition",
        "macro_rules! m { ($($x:expr),*) => {{ use std::{fs}; $( let _ = fs::read($x); )* }}; }\n",
    )
}

pub(in crate::checks::core_io) fn macro_rule_module_named_by_a_metavariable() -> Inspection {
    lib(
        "macro-rule-module",
        "macro_rules! m {\n    ($m:ident) => {\n        mod $m {\n            pub fn now() {}\n\n            \
         pub fn g() {\n                now();\n            }\n        }\n    };\n}\n",
    )
}

pub(in crate::checks::core_io) fn metavariable_glob() -> Inspection {
    rule("metavariable-glob", "$x:ident", "use std::$x::*;")
}

pub(in crate::checks::core_io) fn rule_block_use_group() -> Inspection {
    lib(
        "rule-block-use-group",
        "macro_rules! m { ($b:block) => { fn f() $b fn g() { use std::{fs}; \
         let _ = fs::read(\"a\"); } }; }\n",
    )
}

pub(in crate::checks::core_io) fn rule_block_std_alias() -> Inspection {
    lib(
        "rule-block-std-alias",
        "macro_rules! m { ($b:block) => { fn f() $b fn g() { use std as s; \
         let _ = s::fs::read(\"a\"); } }; }\n",
    )
}

pub(in crate::checks::core_io) fn rule_lifetime() -> Inspection {
    lib(
        "rule-lifetime",
        "macro_rules! m { ($l:lifetime) => { pub fn g<$l>(x: &$l str) { use std::{fs}; \
         let _ = fs::read(x); } }; }\n",
    )
}

pub(in crate::checks::core_io) fn rule_body_does_not_parse() -> Inspection {
    lib(
        "rule-body-does-not-parse",
        "macro_rules! arms { ($($p:pat => $e:expr),*) => { $($p => $e),* }; }\n",
    )
}

pub(in crate::checks::core_io) fn rule_repetition_builds_a_path() -> Inspection {
    lib(
        "rule-repetition-builds-a-path",
        "macro_rules! m { ($($p:ident)*) => { $($p)::*::read(\"a\") }; }\n\n\
         pub fn f() {\n    let _ = m!(std fs);\n}\n",
    )
}

pub(in crate::checks::core_io) fn rule_defines_its_own_now() -> Inspection {
    lib(
        "rule-defines-its-own-now",
        "macro_rules! ts {\n    ($n:ident) => {\n        pub struct $n(u64);\n\n        \
         impl $n {\n            pub fn now() -> Self {\n                $n(0)\n            }\n\n            \
         pub fn later() -> Self {\n                Self::now()\n            }\n        }\n    };\n}\n\n\
         ts!(Stamp);\n",
    )
}
