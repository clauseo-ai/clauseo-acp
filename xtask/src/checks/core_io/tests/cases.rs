//! The core-io planted cases with sources of their own.

use super::{Repo, lib, run_on, run_repo};
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
