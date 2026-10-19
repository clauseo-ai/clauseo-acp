//! The planted cases of the core-io check and its other tests. Each case builds a scratch repo
//! with an `ARCHITECTURE.md` whose `protocol` row holds core I/O, and the sources of
//! `clauseo-protocol` under `crates/protocol/src/`. The cases with sources of their own are in
//! `tests/cases.rs`, those for `macro_rules!` bodies in `tests/rules.rs`.

mod cases;
pub(super) mod planted;
mod rules;

use cases::*;
use rules::*;

use super::*;
use crate::scratch::Scratch;
use std::fs;

/// A Crates table with one held row (`protocol`, line 8) and one that is not (`xtask`).
const ARCHITECTURE: &str = "# Architecture\n\n## Crates\n\n\
    | Crate | Owns | May depend on | Never depends on | Status |\n\
    |---|---|---|---|---|\n\
    | `xtask` | checks | none of ours | tauri | exists |\n\
    | `protocol` | events | serde | tauri, fs, process | exists |\n";

/// The inputs of one scratch repo besides `ARCHITECTURE.md`.
#[derive(Default)]
struct Repo<'a> {
    /// Paths under `crates/protocol/src/`, with their text.
    files: &'a [(&'a str, &'a str)],
    /// The exception file.
    exceptions: Option<&'a str>,
    /// `crates/protocol/Cargo.toml`.
    manifest: Option<&'a str>,
    /// `ARCHITECTURE.md` in place of `ARCHITECTURE`.
    architecture: Option<&'a str>,
}

/// Writes `repo` into a scratch dir named after `case`; returns the dir.
fn scratch(case: &str, repo: &Repo<'_>) -> Scratch {
    let dir = Scratch::new(&format!("core-io-{case}"));
    let architecture = repo.architecture.unwrap_or(ARCHITECTURE);
    fs::write(dir.join(crates_table::FILE), architecture).expect("write ARCHITECTURE.md");
    fs::create_dir_all(dir.join("crates/protocol")).expect("create the crate dir");
    for (path, text) in repo.files {
        let path = dir.join("crates/protocol/src").join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("create the src dir");
        fs::write(path, text).expect("write the source");
    }
    if let Some(text) = repo.exceptions {
        fs::create_dir_all(dir.join("xtask")).expect("create xtask/");
        fs::write(dir.join(EXCEPTIONS), text).expect("write the exceptions");
    }
    if let Some(text) = repo.manifest {
        fs::write(dir.join("crates/protocol/Cargo.toml"), text).expect("write Cargo.toml");
    }
    dir
}

/// The workspace members of every case: `clauseo-protocol` at `crates/protocol`.
fn members() -> Vec<Member> {
    vec![Member {
        name: "clauseo-protocol".to_owned(),
        dir: "crates/protocol".to_owned(),
        path_deps: Vec::new(),
    }]
}

/// Runs the check on a scratch repo holding `repo`.
fn run_repo(case: &str, repo: &Repo<'_>) -> Inspection {
    let dir = scratch(case, repo);
    run(&Context {
        root: dir.to_path_buf(),
        members: members(),
        load_resolved_graph: false,
    })
}

/// Runs the check on a crate with `files` under `src/`.
fn run_on(case: &str, files: &[(&str, &str)]) -> Inspection {
    run_repo(
        case,
        &Repo {
            files,
            ..Repo::default()
        },
    )
}

/// Runs the check on a crate whose only file is `src/lib.rs`, holding `text`.
fn lib(case: &str, text: &str) -> Inspection {
    run_on(case, &[("lib.rs", text)])
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
    Raw,
}

/// The source of `lib.rs` for one module case.
fn module_source(banned: &Banned, shape: Shape) -> String {
    let Banned {
        module: m,
        item,
        call,
        value,
    } = banned;
    let body = |line: String| format!("pub fn f() {{\n    {line}\n}}\n");
    match shape {
        Shape::Use => format!("use std::{m};\n"),
        Shape::Group => format!("use std::{{{m}, io}};\n"),
        Shape::Nested => format!("use std::{{io, {{{m}::{item}}}}};\n"),
        Shape::StdAlias => format!(
            "use std as s;\n\n{}",
            body(format!("let _ = s::{m}::{call};"))
        ),
        Shape::LeadingColon => body(format!("let _ = ::std::{m}::{call};")),
        Shape::Qualified => body(format!("let _ = std::{m}::{call};")),
        Shape::ExternCrate => format!(
            "extern crate std as x;\n\n{}",
            body(format!("let _ = x::{m}::{call};"))
        ),
        Shape::FnBody => body(format!("use std::{m};\n    let _ = {m}::{call};")),
        Shape::InlineMod => format!("mod inner {{\n    pub use std::{m}::{item};\n}}\n"),
        Shape::Glob => "use std::*;\n".to_owned(),
        Shape::Macro => body(format!("let _ = vec![std::{m}::{call}];")),
        Shape::SelfAlias => format!(
            "use std::{{self as t}};\n\n{}",
            body(format!("let _ = t::{m}::{call};"))
        ),
        Shape::Rename => {
            format!("use std::{m} as f;\n\npub fn g() {{\n    let _ = f::{call};\n}}\n")
        }
        Shape::Value => body(format!("let g = std::{m}::{value};\n    drop(g);")),
        Shape::Raw => body(format!("let _ = std::r#{m}::{call};")),
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
    fs_value = FS Value; fs_raw = FS Raw;
    net_use = NET Use; net_group = NET Group; net_nested = NET Nested;
    net_std_alias = NET StdAlias; net_leading_colon = NET LeadingColon;
    net_qualified = NET Qualified; net_extern_crate = NET ExternCrate; net_fn_body = NET FnBody;
    net_inline_mod = NET InlineMod; net_glob = NET Glob; net_macro = NET Macro;
    net_self_alias = NET SelfAlias; net_rename = NET Rename; net_value = NET Value;
    net_raw = NET Raw;
    process_use = PROCESS Use; process_group = PROCESS Group; process_nested = PROCESS Nested;
    process_std_alias = PROCESS StdAlias; process_leading_colon = PROCESS LeadingColon;
    process_qualified = PROCESS Qualified; process_extern_crate = PROCESS ExternCrate;
    process_fn_body = PROCESS FnBody; process_inline_mod = PROCESS InlineMod;
    process_glob = PROCESS Glob; process_macro = PROCESS Macro;
    process_self_alias = PROCESS SelfAlias; process_rename = PROCESS Rename;
    process_value = PROCESS Value; process_raw = PROCESS Raw;
    env_use = ENV Use; env_group = ENV Group; env_nested = ENV Nested;
    env_std_alias = ENV StdAlias; env_leading_colon = ENV LeadingColon;
    env_qualified = ENV Qualified; env_extern_crate = ENV ExternCrate; env_fn_body = ENV FnBody;
    env_inline_mod = ENV InlineMod; env_glob = ENV Glob; env_macro = ENV Macro;
    env_self_alias = ENV SelfAlias; env_rename = ENV Rename; env_value = ENV Value;
    env_raw = ENV Raw;
}

/// The ways a clock case reaches `<clock>::now`.
#[derive(Clone, Copy)]
enum ClockShape {
    Use,
    Rename,
    TypeAlias,
    Qualified,
    Glob,
    SelfInImpl,
    ParenAlias,
}

/// The source of `lib.rs` for one clock case.
fn clock_source(clock: &str, shape: ClockShape) -> String {
    let call = |line: &str| format!("\npub fn f() {{\n    let _ = {line};\n}}\n");
    match shape {
        ClockShape::Use => format!(
            "use std::time::{clock};\n{}",
            call(&format!("{clock}::now()"))
        ),
        ClockShape::Rename => format!("use std::time::{clock} as I;\n{}", call("I::now()")),
        ClockShape::TypeAlias => format!("type T = std::time::{clock};\n{}", call("T::now()")),
        ClockShape::Qualified => call(&format!("<std::time::{clock}>::now()"))
            .trim_start()
            .to_owned(),
        ClockShape::Glob => "use std::time::*;\n".to_owned(),
        ClockShape::SelfInImpl => format!(
            "pub trait N {{\n    fn n() -> Self;\n}}\n\nimpl N for std::time::{clock} {{\n    \
             fn n() -> Self {{\n        Self::now()\n    }}\n}}\n"
        ),
        ClockShape::ParenAlias => format!("type T = (std::time::{clock});\n{}", call("T::now()")),
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
    instant_glob = "Instant" Glob; instant_self_in_impl = "Instant" SelfInImpl;
    instant_paren_alias = "Instant" ParenAlias;
    system_time_use = "SystemTime" Use; system_time_rename = "SystemTime" Rename;
    system_time_type_alias = "SystemTime" TypeAlias;
    system_time_qualified = "SystemTime" Qualified; system_time_glob = "SystemTime" Glob;
    system_time_self_in_impl = "SystemTime" SelfInImpl;
    system_time_paren_alias = "SystemTime" ParenAlias;
}

#[test]
fn every_row_counts_when_no_member_is_held() {
    let dir = Scratch::new("core-io-count");
    fs::write(dir.join(crates_table::FILE), ARCHITECTURE).expect("write ARCHITECTURE.md");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
        load_resolved_graph: false,
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
            load_resolved_graph: false,
        }),
        Inspection::default()
    );
}

#[test]
fn module_paths_follow_the_file_layout() {
    assert_eq!(files::module_of("lib.rs"), Vec::<String>::new());
    assert_eq!(files::module_of("main.rs"), Vec::<String>::new());
    assert_eq!(files::module_of("a.rs"), ["a"]);
    assert_eq!(files::module_of("a/mod.rs"), ["a"]);
    assert_eq!(files::module_of("a/b.rs"), ["a", "b"]);
}

#[test]
fn sites_of_one_kind_are_reported_once_per_line() {
    let found = lib(
        "lines",
        "use std::{\n    io,\n    fs::File,\n};\n\npub fn f() {\n    \
         let _ = <std::fs::File>::open(\"a\");\n    \
         let _ = std::fs::read(\"a\"); let _ = std::fs::read(\"b\");\n}\n",
    );
    let lines: Vec<u32> = found.violations.iter().map(|v| v.line).collect();
    assert_eq!(lines, [3, 7, 8]);
}

#[cfg(unix)]
#[test]
fn a_symlink_under_src_fails_at_the_crate_s_row() {
    let dir = scratch(
        "symlink",
        &Repo {
            files: &[("lib.rs", "mod io;\n")],
            ..Repo::default()
        },
    );
    fs::write(dir.join("crates/protocol/io.rs"), "use std::fs;\n").expect("write io.rs");
    std::os::unix::fs::symlink("../io.rs", dir.join("crates/protocol/src/io.rs"))
        .expect("make the symlink");
    let found: Vec<String> = run(&Context {
        root: dir.to_path_buf(),
        members: members(),
        load_resolved_graph: false,
    })
    .violations
    .iter()
    .map(ToString::to_string)
    .collect();
    assert_eq!(
        found,
        [
            "ARCHITECTURE.md:8: `crates/protocol/src/io.rs` in clauseo-protocol is a symlink, \
             which core-io does not follow\n    fix: replace the symlink with the file or folder \
             it points to, so the code sits under `crates/protocol/src/`"
        ]
    );
}
