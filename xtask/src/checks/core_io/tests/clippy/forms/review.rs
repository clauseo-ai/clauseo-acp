//! The forms the review planted: the fourteen of comment 4312108656 that clippy caught on a
//! scratch crate, the three that still got past the syn check in comment 4312109936, and the
//! macro shapes from 4312108656 that the syn check failed with nothing banned in them.

use super::{Form, fail, pass};

pub(super) fn forms() -> Vec<Form> {
    let mut all = fourteen();
    all.extend(three());
    all.extend(shapes());
    all
}

/// The review's fourteen (4312108656): renames, a type alias, the associated type in a bound, a
/// fn pointer, a re-export, and four macros, `$a::$b::read` and a `use` in a rule body among
/// them.
fn fourteen() -> Vec<Form> {
    vec![
        fail(
            "review_rename_module",
            "use std::fs as files;\npub fn f() {\n    let _ = files::read(\"x\");\n}\n",
        ),
        fail(
            "review_rename_std",
            "use std as s;\npub fn f() {\n    let _ = s::fs::read(\"x\");\n}\n",
        ),
        fail(
            "review_rename_in_a_group",
            "use std::{fs::read as load};\npub fn f() {\n    let _ = load(\"x\");\n}\n",
        ),
        fail(
            "review_rename_type",
            "use std::process::Command as Cmd;\npub fn f() {\n    let _ = Cmd::new(\"x\");\n}\n",
        ),
        fail(
            "review_rename_clock",
            "use std::time::Instant as I;\npub fn f() {\n    let _ = I::now();\n}\n",
        ),
        fail(
            "review_rename_extern_crate",
            "extern crate std as the_std;\npub fn f() {\n    let _ = the_std::env::var(\"x\");\n}\n",
        ),
        fail(
            "review_type_alias",
            "pub type Clock = std::time::Instant;\npub fn f() {\n    let _ = Clock::now();\n}\n",
        ),
        fail(
            "review_associated_type_in_a_bound",
            "pub trait Source {\n    type Clock;\n}\n\
             pub fn f<T: Source<Clock = std::time::Instant>>() {\n    \
             let _ = <T as Source>::Clock::now();\n}\n",
        ),
        fail(
            "review_fn_pointer",
            "pub fn f() {\n    \
             let read: fn(&'static str) -> std::io::Result<Vec<u8>> = std::fs::read;\n    \
             let _ = read(\"x\");\n}\n",
        ),
        fail(
            "review_reexport",
            "mod io {\n    pub use std::fs::read as load;\n}\npub fn f() {\n    \
             let _ = io::load(\"x\");\n}\n",
        ),
        fail(
            "review_macro_two_metavariables",
            "macro_rules! j {\n    ($a:ident $b:ident) => {\n        $a::$b::read(\"x\")\n    };\n}\n\
             pub fn f() {\n    let _ = j!(std fs);\n}\n",
        ),
        fail(
            "review_macro_use_group_in_a_rule",
            "macro_rules! m {\n    ($x:expr) => {{\n        use std::{fs};\n        fs::read($x)\n    \
             }};\n}\npub fn f() {\n    let _ = m!(\"x\");\n}\n",
        ),
        fail(
            "review_macro_std_alias_in_a_rule",
            "macro_rules! m {\n    ($x:expr) => {{\n        use std as s;\n        s::fs::read($x)\n    \
             }};\n}\npub fn f() {\n    let _ = m!(\"x\");\n}\n",
        ),
        fail(
            "review_macro_path_now",
            "macro_rules! m {\n    ($($p:ident)::+) => {\n        $($p)::+::now()\n    };\n}\n\
             pub fn f() {\n    let _ = m!(std::time::Instant);\n}\n",
        ),
    ]
}

/// The three renames behind a macro of 4312109936 (the `use` in a rule body twice: a module and
/// a clock).
fn three() -> Vec<Form> {
    vec![
        fail(
            "followup_use_in_a_rule_binds_where_called",
            "macro_rules! alias {\n    () => {\n        use std as s;\n    };\n}\npub mod b {\n    \
             alias!();\n    pub fn f() {\n        let _ = s::fs::read(\"x\");\n    }\n}\n",
        ),
        fail(
            "followup_use_in_a_rule_binds_a_clock",
            "macro_rules! alias {\n    () => {\n        use std::time::Instant as I;\n    };\n}\n\
             pub mod b {\n    alias!();\n    pub fn f() {\n        let _ = I::now();\n    }\n}\n",
        ),
        fail(
            "followup_extern_crate_metavariable",
            "macro_rules! m {\n    ($a:ident) => {\n        extern crate $a as s;\n    };\n}\nm!(std);\n\
             pub fn f() {\n    let _ = s::fs::read(\"x\");\n}\n",
        ),
        fail(
            "followup_repetition_of_tokens",
            "macro_rules! call {\n    ($($p:tt),*) => {\n        $($p)*::read(\"x\")\n    };\n}\n\
             pub fn f() {\n    let _ = call!(std, ::, fs);\n}\n",
        ),
    ]
}

/// The three shapes of 4312108656, with nothing banned in them: they must pass.
fn shapes() -> Vec<Form> {
    vec![
        pass(
            "shape_enum_macro",
            "macro_rules! kind {\n    ($name:ident { $($v:ident),* $(,)? }) => {\n        \
             #[derive(Debug, Clone, Copy)]\n        pub enum $name { $($v),* }\n        \
             impl $name {\n            pub const ALL: &[$name] = &[$($name::$v),*];\n            \
             pub fn name(self) -> &'static str {\n                \
             match self { $(Self::$v => stringify!($v)),* }\n            }\n        }\n    };\n}\n\
             kind!(EventKind { Started, Stopped });\n",
        ),
        pass(
            "shape_reexport_macro",
            "pub mod ids {\n    pub struct SessionId;\n}\npub mod events {\n    pub struct Event;\n}\n\
             macro_rules! reexport {\n    ($($m:ident),*) => {\n        $(pub use $m::*;)*\n    };\n}\n\
             reexport!(ids, events);\npub fn f() -> (SessionId, Event, Vec<u8>) {\n    \
             (SessionId, Event, Vec::new())\n}\n",
        ),
        pass(
            "shape_cfg_repetition",
            "macro_rules! unix_only {\n    ($( $item:item )*) => {\n        $( #[cfg(unix)] $item )*\n    \
             };\n}\nunix_only! {\n    pub fn one() -> u8 {\n        1\n    }\n}\n",
        ),
        pass(
            "shape_doc_attribute",
            "macro_rules! documented {\n    ($d:literal, $i:item) => {\n        #[doc = $d]\n        \
             $i\n    };\n}\ndocumented!(\"An id.\", pub struct Id(pub u64););\n",
        ),
    ]
}
