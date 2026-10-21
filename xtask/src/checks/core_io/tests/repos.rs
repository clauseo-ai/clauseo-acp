//! The builders of the core-io planted cases and the scratch repo they share. `scratch` writes
//! the files every case has: `ARCHITECTURE.md`, a root `clippy.toml` and list, manifests that
//! inherit the workspace lints and a library root with the forbid line. The builders of the
//! `clippy.toml`, list, root and row inputs are here; the others are by topic in `repos/`.

mod cases;
mod cfgs;
mod macros;
mod roots;

pub(super) use cases::*;
pub(super) use cfgs::*;
pub(super) use macros::*;
pub(super) use roots::*;

use super::*;
use crate::checks::planted::Repo;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;
use crate::workspace::Target;

/// A Crates table with one held row (`protocol`, line 8), one that is not (`xtask`) and a
/// planned held row (`ids`).
const ARCHITECTURE: &str = "# Architecture\n\n## Crates\n\n\
    | Crate | Owns | May depend on | Never depends on | Status |\n\
    |---|---|---|---|---|\n\
    | `xtask` | checks | none of ours | tauri | exists |\n\
    | `protocol` | events | serde | tauri, fs, process | exists |\n\
    | `ids` | ids | serde | any I/O | planned |\n";

/// The scratch repo's root `clippy.toml`.
const ROOT_TOML: &str = "# root\nmsrv = \"1.99\"\ndisallowed-methods = [\n  \
    { path = \"std::time::Instant::now\", reason = \"the core takes time as input\" },\n]\n";

/// The scratch repo's list.
const LIST_TOML: &str = "disallowed-methods = [\n  \
    { path = \"std::fs::read\", reason = \"no files\" },\n]\n\
    disallowed-types = [\n  { path = \"std::fs::File\", reason = \"no files\", \
    allow-invalid = true },\n]\n";

/// The scratch repo's root `Cargo.toml`.
const ROOT_MANIFEST: &str = "[workspace]\nmembers = [\"crates/*\"]\n\n\
    [workspace.lints.clippy]\ndisallowed_methods = \"deny\"\n";

/// The scratch repo's `crates/protocol/Cargo.toml`.
const CRATE_MANIFEST: &str =
    "[package]\nname = \"clauseo-protocol\"\n\n[lints]\nworkspace = true\n";

/// The library root of `clauseo-protocol`, a target of every case.
const LIB_ROOT: &str = "crates/protocol/src/lib.rs";

/// A target root that starts with the forbid line.
const FORBIDDING_ROOT: &str = "//! A core crate.\n#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]\n\
     #![doc(test(attr(deny(warnings))))]\n\npub fn f() {}\n";

/// The inputs of one scratch repo.
#[derive(Default)]
pub(super) struct Layout<'a> {
    /// `crates/protocol/clippy.toml`, if any.
    crate_file: Option<&'a str>,
    /// More files, written last, so one can replace a manifest: (repo-relative path, text).
    files: &'a [(&'a str, &'a str)],
    /// `ARCHITECTURE.md` in place of `ARCHITECTURE`.
    architecture: Option<&'a str>,
    /// The root `clippy.toml`, `ROOT_TOML` by default.
    root: Input<'a>,
    /// The list, `LIST_TOML` by default.
    list: Input<'a>,
    /// More targets of `clauseo-protocol` beside its library: (kind, repo-relative root).
    targets: &'a [(&'a str, &'a str)],
    /// Symlinks to folders, made after the files: (repo-relative link, target as the link holds
    /// it).
    folder_links: &'a [(&'a str, &'a str)],
}

/// One input file of a scratch repo.
#[derive(Default, Clone, Copy)]
enum Input<'a> {
    /// The scratch repo's usual text.
    #[default]
    Usual,
    /// No file.
    Missing,
    /// This text.
    Text(&'a str),
}

impl<'a> Input<'a> {
    /// The text to write, if any: `usual` for `Usual`.
    fn text(self, usual: &'a str) -> Option<&'a str> {
        match self {
            Input::Usual => Some(usual),
            Input::Missing => None,
            Input::Text(text) => Some(text),
        }
    }
}

/// Writes `repo` into a scratch git repo named after `case`; returns the dir.
pub(super) fn scratch(case: &str, repo: &Layout<'_>) -> Scratch {
    let dir = Scratch::new(&format!("core-io-{case}"));
    let init = Cmd::new(Program::Git, ["init", "-q"], crate::process::TEST_TIMEOUT);
    let quiet = process::run_quiet(&init, &dir).expect("run git init");
    assert!(quiet.success(), "git init failed: {}", quiet.stderr);
    let architecture = repo.architecture.unwrap_or(ARCHITECTURE);
    Scratch::write(dir.join(crates_table::FILE), architecture).expect("write ARCHITECTURE.md");
    Scratch::create_dir_all(dir.join("crates/protocol")).expect("create the crate dir");
    Scratch::create_dir_all(dir.join("xtask")).expect("create xtask/");
    if let Some(text) = repo.root.text(ROOT_TOML) {
        Scratch::write(dir.join(clippy_toml::ROOT), text).expect("write the root clippy.toml");
    }
    if let Some(text) = repo.list.text(LIST_TOML) {
        Scratch::write(dir.join(clippy_toml::LIST), text).expect("write the list");
    }
    if let Some(text) = repo.crate_file {
        Scratch::write(dir.join("crates/protocol/clippy.toml"), text).expect("write clippy.toml");
    }
    let defaults = [
        ("Cargo.toml", ROOT_MANIFEST),
        ("crates/protocol/Cargo.toml", CRATE_MANIFEST),
        (LIB_ROOT, FORBIDDING_ROOT),
    ];
    for (path, text) in defaults.iter().chain(repo.files) {
        let path = dir.join(path);
        Scratch::create_dir_all(path.parent().expect("a file has a parent")).expect("create dirs");
        Scratch::write(path, text).expect("write a file");
    }
    for (link, target) in repo.folder_links {
        Scratch::symlink_dir(target, dir.join(link)).expect("make a symlink");
    }
    dir
}

/// The workspace root the members' targets are placed against, as `cargo metadata` would give
/// it. A target's absolute path then does not depend on where the scratch repo lies.
const METADATA_ROOT: &str = "/ws";

/// The workspace members of every case: `clauseo-protocol` at `crates/protocol`, with its library
/// and `targets`, and `clauseo-extra`, which has no row and no file.
fn members(targets: &[(&str, &str)]) -> Vec<Member> {
    ["protocol", "extra"]
        .into_iter()
        .map(|name| {
            let lib = format!("crates/{name}/src/lib.rs");
            let more = if name == "protocol" { targets } else { &[] };
            Member {
                name: format!("clauseo-{name}"),
                dir: format!("crates/{name}"),
                path_deps: Vec::new(),
                targets: std::iter::once(("lib", lib.as_str()))
                    .chain(more.iter().copied())
                    .map(|(kind, src_path)| {
                        let base = Path::new(METADATA_ROOT);
                        Target::new(vec![kind.to_owned()], &base.join(src_path), base)
                    })
                    .collect(),
                dependencies: crate::workspace::PackageDependencies::default(),
            }
        })
        .collect()
}

/// The scratch repo holding `repo`, with the members `members` gives.
fn run_repo(case: &str, repo: &Layout<'_>) -> Repo {
    let dir = scratch(case, repo);
    Repo::with_members(dir, members(repo.targets))
}

/// The repo with `crate_file` as `crates/protocol/clippy.toml`.
fn with_crate_file(case: &str, crate_file: &str) -> Repo {
    run_repo(
        case,
        &Layout {
            crate_file: Some(crate_file),
            ..Layout::default()
        },
    )
}

/// What `crates/protocol/clippy.toml` must be in the scratch repo, built in a scratch repo of
/// its own named after `case`.
pub(super) fn expected(case: &str) -> Expected {
    let dir = scratch(&format!("{case}-expected"), &Layout::default());
    clippy_toml::load(&dir).expect("the scratch inputs load")
}

pub(in crate::checks::core_io::tests) fn missing_crate_file() -> Repo {
    run_repo("missing-crate-file", &Layout::default())
}

pub(in crate::checks::core_io::tests) fn crate_file_differs() -> Repo {
    with_crate_file("crate-file-differs", ROOT_TOML)
}

pub(in crate::checks::core_io::tests) fn crate_file_matches() -> Repo {
    with_crate_file("crate-file-matches", &expected("crate-file-matches").text)
}

/// The same tables as the generated file, written as arrays of tables, in another key order
/// and with comments.
pub(in crate::checks::core_io::tests) fn crate_file_reformatted() -> Repo {
    with_crate_file(
        "crate-file-reformatted",
        "# written by hand\nmsrv = \"1.99\"\n\
         [[disallowed-types]]\nallow-invalid = true\nreason = \"no files\"\npath = \"std::fs::File\"\n\n\
         [[disallowed-methods]]\npath = \"std::time::Instant::now\"\n\
         reason = \"the core takes time as input\"\n\n\
         [[disallowed-methods]]\npath = \"std::fs::read\" # the list's\nreason = \"no files\"\n",
    )
}

pub(in crate::checks::core_io::tests) fn crate_file_not_toml() -> Repo {
    with_crate_file(
        "crate-file-not-toml",
        "msrv = \"1.99\"\ndisallowed-methods = [\n",
    )
}

/// A row whose Never list is `clock` alone needs no file: the root bans both clocks.
pub(in crate::checks::core_io::tests) fn clock_only_row() -> Repo {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clock-only-row",
        &Layout {
            architecture: Some(&architecture),
            ..Layout::default()
        },
    )
}

/// `clauseo-extra` has no row and no file; with protocol's row gone, nothing is held.
pub(in crate::checks::core_io::tests) fn crate_without_row() -> Repo {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "tauri");
    run_repo(
        "crate-without-row",
        &Layout {
            architecture: Some(&architecture),
            ..Layout::default()
        },
    )
}

pub(in crate::checks::core_io::tests) fn list_missing() -> Repo {
    run_repo(
        "list-missing",
        &Layout {
            list: Input::Missing,
            ..Layout::default()
        },
    )
}

pub(in crate::checks::core_io::tests) fn root_missing() -> Repo {
    run_repo(
        "root-missing",
        &Layout {
            root: Input::Missing,
            ..Layout::default()
        },
    )
}

pub(in crate::checks::core_io::tests) fn list_unknown_key() -> Repo {
    run_repo(
        "list-unknown-key",
        &Layout {
            list: Input::Text(
                "disallowed-methods = [\n  { path = \"std::fs::read\", reason = \"no files\" },\n]\n\
                 disallowed-macros = []\n",
            ),
            ..Layout::default()
        },
    )
}

pub(in crate::checks::core_io::tests) fn list_entry_problems() -> Repo {
    run_repo(
        "list-entry-problems",
        &Layout {
            list: Input::Text(
                "disallowed-methods = [\n  { path = \"std::fs::read\", reason = \"no files\" },\n  \
                 { path = \"std::fs::read\", reason = \" \" },\n]\n",
            ),
            ..Layout::default()
        },
    )
}

/// A root `clippy.toml` that is not TOML: the message names the root file and its fix.
pub(in crate::checks::core_io::tests) fn root_not_toml() -> Repo {
    run_repo(
        "root-not-toml",
        &Layout {
            root: Input::Text("msrv = \"1.99\"\ndisallowed-methods = [\n"),
            ..Layout::default()
        },
    )
}

pub(in crate::checks::core_io::tests) fn root_array_is_a_string() -> Repo {
    run_repo(
        "root-array-is-a-string",
        &Layout {
            root: Input::Text("msrv = \"1.99\"\ndisallowed-types = \"std::fs::File\"\n"),
            ..Layout::default()
        },
    )
}
