//! The planted cases of the core-io check and its other tests. Each case builds a scratch repo
//! with an `ARCHITECTURE.md` whose `protocol` row holds core I/O, and the sources of
//! `clauseo-protocol` under `crates/protocol/src/`.

use super::*;
use crate::scratch::Scratch;

/// A Crates table with one held row (`protocol`, line 8) and one that is not (`xtask`).
const ARCHITECTURE: &str = "# Architecture\n\n## Crates\n\n\
    | Crate | Owns | May depend on | Never depends on | Status |\n\
    |---|---|---|---|---|\n\
    | `xtask` | checks | none of ours | tauri | exists |\n\
    | `protocol` | events | serde | tauri, fs, process | exists |\n";

/// Runs the check on a scratch repo with `files` (paths under `crates/protocol/src/`, text) and,
/// when given, `exceptions` as the exception file.
fn run_on(case: &str, files: &[(&str, &str)], exceptions: Option<&str>) -> Inspection {
    let dir = Scratch::new(&format!("core-io-{case}"));
    fs::write(dir.join(crates_table::FILE), ARCHITECTURE).expect("write ARCHITECTURE.md");
    for (path, text) in files {
        let path = dir.join("crates/protocol/src").join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("create the src dir");
        fs::write(path, text).expect("write the source");
    }
    if let Some(text) = exceptions {
        fs::create_dir_all(dir.join("xtask")).expect("create xtask/");
        fs::write(dir.join(EXCEPTIONS), text).expect("write the exceptions");
    }
    run(&Context {
        root: dir.to_path_buf(),
        members: vec![Member {
            name: "clauseo-protocol".to_owned(),
            dir: "crates/protocol".to_owned(),
            path_deps: Vec::new(),
        }],
    })
}

/// Runs the check on a crate whose only file is `src/lib.rs`, holding `text`.
fn lib(case: &str, text: &str) -> Inspection {
    run_on(case, &[("lib.rs", text)], None)
}

/// A banned module and an item of it, for the module cases.
struct Banned {
    /// `fs`.
    module: &'static str,
    /// An item to import: `read`.
    item: &'static str,
    /// A call of an item, as written after `std::<module>::`: `read("a")`.
    call: &'static str,
    /// A path to name as a value, after `std::<module>::`: `read`.
    value: &'static str,
}

const FS: Banned = Banned {
    module: "fs",
    item: "read",
    call: "read(\"a\")",
    value: "read",
};
const NET: Banned = Banned {
    module: "net",
    item: "TcpStream",
    call: "TcpStream::connect(\"a:1\")",
    value: "TcpStream::connect",
};
const PROCESS: Banned = Banned {
    module: "process",
    item: "Command",
    call: "Command::new(\"a\")",
    value: "Command::new",
};
const ENV: Banned = Banned {
    module: "env",
    item: "var",
    call: "var(\"a\")",
    value: "var",
};

/// The ways a module case reaches the banned module.
#[derive(Clone, Copy)]
enum Shape {
    Use,
    Group,
    Nested,
    StdAlias,
    LeadingColon,
    Qualified,
    ExternCrate,
    FnBody,
    InlineMod,
    Glob,
    Macro,
    SelfAlias,
    Rename,
    Value,
}

/// The source of `lib.rs` for one module case.
fn module_source(banned: &Banned, shape: Shape) -> String {
    let Banned {
        module: m,
        item,
        call,
        value,
    } = banned;
    match shape {
        Shape::Use => format!("use std::{m};\n"),
        Shape::Group => format!("use std::{{{m}, io}};\n"),
        Shape::Nested => format!("use std::{{io, {{{m}::{item}}}}};\n"),
        Shape::StdAlias => {
            format!("use std as s;\n\npub fn f() {{\n    let _ = s::{m}::{call};\n}}\n")
        }
        Shape::LeadingColon => format!("pub fn f() {{\n    let _ = ::std::{m}::{call};\n}}\n"),
        Shape::Qualified => format!("pub fn f() {{\n    let _ = std::{m}::{call};\n}}\n"),
        Shape::ExternCrate => {
            format!("extern crate std as x;\n\npub fn f() {{\n    let _ = x::{m}::{call};\n}}\n")
        }
        Shape::FnBody => {
            format!("pub fn f() {{\n    use std::{m};\n    let _ = {m}::{call};\n}}\n")
        }
        Shape::InlineMod => format!("mod inner {{\n    pub use std::{m}::{item};\n}}\n"),
        Shape::Glob => "use std::*;\n".to_owned(),
        Shape::Macro => format!("pub fn f() {{\n    let _ = vec![std::{m}::{call}];\n}}\n"),
        Shape::SelfAlias => {
            format!("use std::{{self as t}};\n\npub fn f() {{\n    let _ = t::{m}::{call};\n}}\n")
        }
        Shape::Rename => {
            format!("use std::{m} as f;\n\npub fn g() {{\n    let _ = f::{call};\n}}\n")
        }
        Shape::Value => {
            format!("pub fn f() {{\n    let g = std::{m}::{value};\n    drop(g);\n}}\n")
        }
    }
}

/// Makes one case function per `name = BANNED Shape;` line.
macro_rules! module_cases {
    ($($name:ident = $banned:ident $shape:ident;)+) => {$(
        pub(super) fn $name() -> Inspection {
            lib(stringify!($name), &module_source(&$banned, Shape::$shape))
        }
    )+};
}

module_cases! {
    fs_use = FS Use; fs_group = FS Group; fs_nested = FS Nested; fs_std_alias = FS StdAlias;
    fs_leading_colon = FS LeadingColon; fs_qualified = FS Qualified;
    fs_extern_crate = FS ExternCrate; fs_fn_body = FS FnBody; fs_inline_mod = FS InlineMod;
    fs_glob = FS Glob; fs_macro = FS Macro; fs_self_alias = FS SelfAlias; fs_rename = FS Rename;
    fs_value = FS Value;
    net_use = NET Use; net_group = NET Group; net_nested = NET Nested;
    net_std_alias = NET StdAlias; net_leading_colon = NET LeadingColon;
    net_qualified = NET Qualified; net_extern_crate = NET ExternCrate; net_fn_body = NET FnBody;
    net_inline_mod = NET InlineMod; net_glob = NET Glob; net_macro = NET Macro;
    net_self_alias = NET SelfAlias; net_rename = NET Rename; net_value = NET Value;
    process_use = PROCESS Use; process_group = PROCESS Group; process_nested = PROCESS Nested;
    process_std_alias = PROCESS StdAlias; process_leading_colon = PROCESS LeadingColon;
    process_qualified = PROCESS Qualified; process_extern_crate = PROCESS ExternCrate;
    process_fn_body = PROCESS FnBody; process_inline_mod = PROCESS InlineMod;
    process_glob = PROCESS Glob; process_macro = PROCESS Macro;
    process_self_alias = PROCESS SelfAlias; process_rename = PROCESS Rename;
    process_value = PROCESS Value;
    env_use = ENV Use; env_group = ENV Group; env_nested = ENV Nested;
    env_std_alias = ENV StdAlias; env_leading_colon = ENV LeadingColon;
    env_qualified = ENV Qualified; env_extern_crate = ENV ExternCrate; env_fn_body = ENV FnBody;
    env_inline_mod = ENV InlineMod; env_glob = ENV Glob; env_macro = ENV Macro;
    env_self_alias = ENV SelfAlias; env_rename = ENV Rename; env_value = ENV Value;
}

/// The ways a clock case reaches `<clock>::now`.
#[derive(Clone, Copy)]
enum ClockShape {
    Use,
    Rename,
    TypeAlias,
    Qualified,
    Glob,
}

/// The source of `lib.rs` for one clock case.
fn clock_source(clock: &str, shape: ClockShape) -> String {
    match shape {
        ClockShape::Use => {
            format!("use std::time::{clock};\n\npub fn f() {{\n    let _ = {clock}::now();\n}}\n")
        }
        ClockShape::Rename => {
            format!("use std::time::{clock} as I;\n\npub fn f() {{\n    let _ = I::now();\n}}\n")
        }
        ClockShape::TypeAlias => {
            format!("type T = std::time::{clock};\n\npub fn f() {{\n    let _ = T::now();\n}}\n")
        }
        ClockShape::Qualified => {
            format!("pub fn f() {{\n    let _ = <std::time::{clock}>::now();\n}}\n")
        }
        ClockShape::Glob => "use std::time::*;\n".to_owned(),
    }
}

/// Makes one case function per `name = "Clock" Shape;` line.
macro_rules! clock_cases {
    ($($name:ident = $clock:literal $shape:ident;)+) => {$(
        pub(super) fn $name() -> Inspection {
            lib(stringify!($name), &clock_source($clock, ClockShape::$shape))
        }
    )+};
}

clock_cases! {
    instant_use = "Instant" Use; instant_rename = "Instant" Rename;
    instant_type_alias = "Instant" TypeAlias; instant_qualified = "Instant" Qualified;
    instant_glob = "Instant" Glob;
    system_time_use = "SystemTime" Use; system_time_rename = "SystemTime" Rename;
    system_time_type_alias = "SystemTime" TypeAlias;
    system_time_qualified = "SystemTime" Qualified; system_time_glob = "SystemTime" Glob;
}

pub(super) fn use_then_call() -> Inspection {
    lib(
        "use-then-call",
        "use std::fs;\n\npub fn f() {\n    let _ = fs::read(\"a\");\n}\n",
    )
}

pub(super) fn alias_in_another_file() -> Inspection {
    run_on(
        "alias-in-another-file",
        &[
            (
                "lib.rs",
                "mod clock;\n\npub fn f() {\n    let _ = clock::Clock::now();\n}\n",
            ),
            ("clock.rs", "pub type Clock = std::time::Instant;\n"),
        ],
        None,
    )
}

pub(super) fn std_reexported_from_the_root() -> Inspection {
    run_on(
        "std-reexported",
        &[
            ("lib.rs", "pub use std as s;\nmod a;\n"),
            (
                "a.rs",
                "pub fn f() {\n    let _ = crate::s::fs::read(\"a\");\n}\n",
            ),
        ],
        None,
    )
}

pub(super) fn does_not_parse() -> Inspection {
    lib("does-not-parse", "pub fn f() {}\n\npub fn g( {\n")
}

pub(super) fn no_sources() -> Inspection {
    run_on("no-sources", &[], None)
}

pub(super) fn stale_exception() -> Inspection {
    run_on(
        "stale-exception",
        &[("lib.rs", "pub fn f() {}\n")],
        Some("# header\ncrates/protocol/src/gone.rs  reads fixtures\n"),
    )
}

pub(super) fn allowed_items() -> Inspection {
    lib(
        "allowed-items",
        "use std::io::Error;\nuse std::time::Duration;\n\n\
         pub fn f(d: Duration) -> Error {\n    let _ = std::time::Duration::from_secs(1);\n    \
         Error::other(format!(\"{d:?}\"))\n}\n",
    )
}

pub(super) fn instant_as_a_type() -> Inspection {
    lib(
        "instant-as-a-type",
        "use std::time::Instant;\n\n\
         pub fn elapsed(start: Instant, end: std::time::Instant) -> std::time::Duration {\n    \
         end - start\n}\n",
    )
}

pub(super) fn listed_exception() -> Inspection {
    run_on(
        "listed-exception",
        &[("lib.rs", "use std::fs;\n")],
        Some("crates/protocol/src/lib.rs  reads test fixtures\n"),
    )
}

pub(super) fn local_names_like_banned_ones() -> Inspection {
    lib(
        "local-names",
        "mod fs {\n    pub fn read(_: &str) {}\n}\n\npub fn f() {\n    fs::read(\"a\");\n    \
         let _ = other::time::Instant::now();\n}\n",
    )
}

#[test]
fn every_row_counts_and_a_planned_held_row_is_not_checked() {
    let dir = Scratch::new("core-io-count");
    fs::write(dir.join(crates_table::FILE), ARCHITECTURE).expect("write ARCHITECTURE.md");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    });
    assert_eq!(found, Inspection::new(2, Vec::new()));
}

#[test]
fn a_missing_table_inspects_nothing() {
    let dir = Scratch::new("core-io-no-table");
    assert_eq!(
        run(&Context {
            root: dir.to_path_buf(),
            members: Vec::new(),
        }),
        Inspection::default()
    );
}

#[test]
fn module_paths_follow_the_file_layout() {
    assert_eq!(module_of("lib.rs"), Vec::<String>::new());
    assert_eq!(module_of("main.rs"), Vec::<String>::new());
    assert_eq!(module_of("a.rs"), ["a"]);
    assert_eq!(module_of("a/mod.rs"), ["a"]);
    assert_eq!(module_of("a/b.rs"), ["a", "b"]);
}

#[test]
fn each_site_is_reported_once_at_its_own_line() {
    let found = lib(
        "lines",
        "use std::{\n    io,\n    fs::File,\n};\n\npub fn f() {\n    let _ = <std::fs::File>::open(\"a\");\n    \
         let _ = std::fs::read(\"a\"); let _ = std::fs::read(\"b\");\n}\n",
    );
    let lines: Vec<u32> = found.violations.iter().map(|v| v.line).collect();
    assert_eq!(lines, [3, 7, 8, 8]);
}

#[test]
fn zz_dump() {
    for case in super::PLANTED {
        let found = (case.run)();
        eprintln!("== {} ({})", case.name, found.inspected);
        for v in &found.violations {
            eprintln!("{v}");
        }
    }
}

#[test]
fn the_check_is_registered_under_its_name() {
    assert!(
        crate::checks::ALL
            .iter()
            .any(|check| check.name == "core-io")
    );
}
