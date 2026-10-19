//! The core-io planted cases with sources of their own.

use super::{ARCHITECTURE, Repo, lib, run_on, run_repo};
use crate::checks::Inspection;

pub(in crate::checks::core_io) fn raw_std() -> Inspection {
    lib(
        "raw-std",
        "pub fn f() {\n    let _ = r#std::fs::read(\"a\");\n}\n",
    )
}

pub(in crate::checks::core_io) fn raw_use() -> Inspection {
    lib("raw-use", "use std::r#fs;\n")
}

pub(in crate::checks::core_io) fn instant_raw_now() -> Inspection {
    lib(
        "instant-raw-now",
        "use std::time::Instant;\n\npub fn f() {\n    let _ = Instant::r#now();\n}\n",
    )
}

/// A trait `K` with an associated type `C`, and `Z`, whose `C` is `Instant`; then `rest`.
fn associated(case: &str, rest: &str) -> Inspection {
    lib(
        case,
        &format!(
            "pub trait K {{\n    type C;\n    fn t();\n}}\n\npub struct Z;\n\nimpl K for Z {{\n    \
             type C = std::time::Instant;\n{rest}}}\n"
        ),
    )
}

pub(in crate::checks::core_io) fn associated_type_qualified() -> Inspection {
    associated(
        "associated-qualified",
        "    fn t() {}\n}\n\npub fn f() {\n    let _ = <Z as K>::C::now();\n",
    )
}

pub(in crate::checks::core_io) fn associated_type_self() -> Inspection {
    associated(
        "associated-self",
        "    fn t() {\n        let _ = Self::C::now();\n    }\n",
    )
}

pub(in crate::checks::core_io) fn os_unix_fs() -> Inspection {
    lib(
        "os-unix-fs",
        "pub fn f() {\n    let _ = std::os::unix::fs::symlink(\"a\", \"b\");\n}\n",
    )
}

pub(in crate::checks::core_io) fn os_unix_net() -> Inspection {
    lib("os-unix-net", "use std::os::unix::net::UnixStream;\n")
}

pub(in crate::checks::core_io) fn os_unix_process() -> Inspection {
    lib(
        "os-unix-process",
        "use std::os::unix::process::CommandExt;\n",
    )
}

pub(in crate::checks::core_io) fn os_windows_fs() -> Inspection {
    lib(
        "os-windows-fs",
        "pub fn f() {\n    let _ = std::os::windows::fs::symlink_file(\"a\", \"b\");\n}\n",
    )
}

pub(in crate::checks::core_io) fn os_glob() -> Inspection {
    lib("os-glob", "use std::os::*;\n")
}

pub(in crate::checks::core_io) fn os_unix_glob() -> Inspection {
    lib("os-unix-glob", "use std::os::unix::*;\n")
}

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

pub(in crate::checks::core_io) fn macro_block_use() -> Inspection {
    lib(
        "macro-block-use",
        "pub fn f() {\n    let _ = vec![{\n        use std::time::Instant as I;\n        \
         I::now()\n    }];\n}\n",
    )
}

pub(in crate::checks::core_io) fn macro_rules_item() -> Inspection {
    lib(
        "macro-rules-item",
        "macro_rules! a {\n    () => {\n        type U = std::time::Instant;\n    };\n}\n\n\
         a!();\n\npub fn f() {\n    let _ = U::now();\n}\n",
    )
}

pub(in crate::checks::core_io) fn extern_crate_self() -> Inspection {
    lib(
        "extern-crate-self",
        "extern crate self as me;\npub use std as s;\n\npub fn f() {\n    \
         let _ = me::s::fs::read(\"a\");\n}\n",
    )
}

pub(in crate::checks::core_io) fn bin_crate_root() -> Inspection {
    run_on(
        "bin-crate-root",
        &[
            ("lib.rs", "pub fn g() {}\n"),
            (
                "bin/x.rs",
                "use std as s;\n\nmod m {\n    pub fn f() {\n        \
                 let _ = crate::s::fs::read(\"a\");\n    }\n}\n\nfn main() {}\n",
            ),
        ],
    )
}

pub(in crate::checks::core_io) fn main_crate_root() -> Inspection {
    run_on(
        "main-crate-root",
        &[
            ("lib.rs", "pub fn g() {}\n"),
            ("main.rs", "use std as s;\n\nmod m;\n\nfn main() {}\n"),
            (
                "m.rs",
                "pub fn f() {\n    let _ = crate::s::fs::read(\"a\");\n}\n",
            ),
        ],
    )
}

pub(in crate::checks::core_io) fn use_then_call() -> Inspection {
    lib(
        "use-then-call",
        "use std::fs;\n\npub fn f() {\n    let _ = fs::read(\"a\");\n}\n",
    )
}

pub(in crate::checks::core_io) fn alias_in_another_file() -> Inspection {
    run_on(
        "alias-in-another-file",
        &[
            (
                "lib.rs",
                "mod clock;\n\npub fn f() {\n    let _ = clock::Clock::now();\n}\n",
            ),
            ("clock.rs", "pub type Clock = std::time::Instant;\n"),
        ],
    )
}

pub(in crate::checks::core_io) fn std_reexported_from_the_root() -> Inspection {
    run_on(
        "std-reexported",
        &[
            ("lib.rs", "pub use std as s;\nmod a;\n"),
            (
                "a.rs",
                "pub fn f() {\n    let _ = crate::s::fs::read(\"a\");\n}\n",
            ),
        ],
    )
}

pub(in crate::checks::core_io) fn path_attribute() -> Inspection {
    lib("path-attribute", "#[path = \"../io.rs\"]\nmod io;\n")
}

pub(in crate::checks::core_io) fn include_macro() -> Inspection {
    lib("include-macro", "include!(\"../io.rs\");\n")
}

/// A crate whose `Cargo.toml` is `manifest`, with one file under `src/`.
fn with_manifest(case: &str, manifest: &str) -> Inspection {
    run_repo(
        case,
        &Repo {
            files: &[("x.rs", "pub fn f() {}\n")],
            manifest: Some(manifest),
            ..Repo::default()
        },
    )
}

pub(in crate::checks::core_io) fn lib_path_outside_src() -> Inspection {
    with_manifest(
        "lib-path",
        "[package]\nname = \"clauseo-protocol\"\n\n[lib]\npath = \"lib.rs\"\n",
    )
}

pub(in crate::checks::core_io) fn bin_path_outside_src() -> Inspection {
    with_manifest(
        "bin-path",
        "[package]\nname = \"clauseo-protocol\"\n\n[[bin]]\nname = \"tool\"\n\
         path = \"tools/main.rs\"\n",
    )
}

pub(in crate::checks::core_io) fn does_not_parse() -> Inspection {
    lib("does-not-parse", "pub fn f() {}\n\npub fn g( {\n")
}

pub(in crate::checks::core_io) fn no_sources() -> Inspection {
    run_on("no-sources", &[])
}

pub(in crate::checks::core_io) fn stale_exception() -> Inspection {
    run_repo(
        "stale-exception",
        &Repo {
            files: &[("lib.rs", "pub fn f() {}\n")],
            exceptions: Some("# header\ncrates/protocol/src/gone.rs  reads fixtures\n"),
            ..Repo::default()
        },
    )
}

pub(in crate::checks::core_io) fn exception_outside_held_crates() -> Inspection {
    run_repo(
        "exception-outside",
        &Repo {
            files: &[("lib.rs", "pub fn f() {}\n")],
            exceptions: Some("xtask/core-io-exceptions.txt  lists itself\n"),
            ..Repo::default()
        },
    )
}

pub(in crate::checks::core_io) fn allowed_items() -> Inspection {
    lib(
        "allowed-items",
        "use std::io::Error;\nuse std::time::Duration;\n\n\
         pub fn f(d: Duration) -> Error {\n    let _ = std::time::Duration::from_secs(1);\n    \
         Error::other(format!(\"{d:?}\"))\n}\n",
    )
}

pub(in crate::checks::core_io) fn instant_as_a_type() -> Inspection {
    lib(
        "instant-as-a-type",
        "use std::time::Instant;\n\n\
         pub fn elapsed(start: Instant, end: std::time::Instant) -> std::time::Duration {\n    \
         end - start\n}\n",
    )
}

pub(in crate::checks::core_io) fn listed_exception() -> Inspection {
    run_repo(
        "listed-exception",
        &Repo {
            files: &[("lib.rs", "use std::fs;\n")],
            exceptions: Some("crates/protocol/src/lib.rs  reads test fixtures\n"),
            ..Repo::default()
        },
    )
}

pub(in crate::checks::core_io) fn local_names_like_banned_ones() -> Inspection {
    lib(
        "local-names",
        "mod fs {\n    pub fn read(_: &str) {}\n}\n\npub fn f() {\n    fs::read(\"a\");\n    \
         let _ = other::time::Instant::now();\n}\n",
    )
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

pub(in crate::checks::core_io) fn include_alias() -> Inspection {
    lib(
        "include-alias",
        "use std::include as inc;\n\ninc!(\"../io.rs\");\n",
    )
}

pub(in crate::checks::core_io) fn core_include() -> Inspection {
    lib("core-include", "core::include!(\"../io.rs\");\n")
}

/// A row whose Never list is `clock` alone: `std::fs` passes, `Instant::now` fails.
pub(in crate::checks::core_io) fn clock_only_row() -> Inspection {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clock-only-row",
        &Repo {
            files: &[(
                "lib.rs",
                "pub fn f() {\n    let _ = std::fs::read(\"a\");\n    \
                 let _ = std::time::Instant::now();\n}\n",
            )],
            architecture: Some(&architecture),
            ..Repo::default()
        },
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

pub(in crate::checks::core_io) fn associated_type_in_a_bound() -> Inspection {
    lib(
        "associated-type-in-a-bound",
        "pub trait K {\n    type C;\n}\n\n\
         pub fn f<T: K<C = std::time::Instant>>() {\n    let _ = <T::C>::now();\n}\n",
    )
}

pub(in crate::checks::core_io) fn associated_name_like_a_local_type() -> Inspection {
    lib(
        "associated-name-like-a-local-type",
        "pub trait K {\n    type Output;\n}\n\npub struct Z;\n\nimpl K for Z {\n    \
         type Output = std::time::Instant;\n}\n\npub struct Output;\n\nimpl Output {\n    \
         pub fn now() -> Output {\n        Output\n    }\n}\n\n\
         pub fn f() {\n    let _ = Output::now();\n}\n",
    )
}

pub(in crate::checks::core_io) fn core_net() -> Inspection {
    lib(
        "core-net",
        "pub fn f() -> core::net::Ipv4Addr {\n    core::net::Ipv4Addr::LOCALHOST\n}\n",
    )
}

pub(in crate::checks::core_io) fn std_env_macro() -> Inspection {
    lib(
        "std-env-macro",
        "pub fn f() -> &'static str {\n    std::env!(\"CARGO_PKG_NAME\")\n}\n",
    )
}

pub(in crate::checks::core_io) fn cfg_attr_on_a_mod() -> Inspection {
    run_on(
        "cfg-attr-on-a-mod",
        &[
            ("lib.rs", "pub fn g() {}\n"),
            (
                "main.rs",
                "use std as s;\n\n#[cfg_attr(test, allow(dead_code))]\nmod m;\n\nfn main() {}\n",
            ),
            (
                "m.rs",
                "pub fn f() {\n    let _ = crate::s::fs::read(\"a\");\n}\n",
            ),
        ],
    )
}

pub(in crate::checks::core_io) fn os_prelude_glob() -> Inspection {
    lib("os-prelude-glob", "use std::os::unix::prelude::*;\n")
}

pub(in crate::checks::core_io) fn macro_rule_visibility() -> Inspection {
    lib(
        "macro-rule-visibility",
        "macro_rules! m {
    ($vis:vis) => {
        $vis fn f() {
                     use std::{fs};
            let _ = fs::read(\"a\");
        }
    };
}
",
    )
}

pub(in crate::checks::core_io) fn macro_rule_repetition() -> Inspection {
    lib(
        "macro-rule-repetition",
        "macro_rules! m { ($($x:expr),*) => {{ use std::{fs}; $( let _ = fs::read($x); )* }}; }
",
    )
}

pub(in crate::checks::core_io) fn macro_rule_module_named_by_a_metavariable() -> Inspection {
    lib(
        "macro-rule-module",
        "macro_rules! m {
    ($m:ident) => {
        mod $m {
                     pub fn now() {}

            pub fn g() {
                now();
            }
                 }
    };
}
",
    )
}

pub(in crate::checks::core_io) fn metavariable_glob() -> Inspection {
    rule("metavariable-glob", "$x:ident", "use std::$x::*;")
}

pub(in crate::checks::core_io) fn cfg_attr_nested_path() -> Inspection {
    lib(
        "cfg-attr-nested-path",
        "#[cfg_attr(unix, cfg_attr(test, path = \"../io.rs\"))]\nmod io;\n",
    )
}
