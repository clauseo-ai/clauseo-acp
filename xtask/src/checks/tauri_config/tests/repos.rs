//! The builders of the tauri-config planted cases. Each writes a desktop crate into a scratch repo:
//! a valid `tauri.conf.json` (`CONF`) and `build.rs` (`BUILD`), with one thing changed. The
//! shared helpers and the repos with no config are here; the rest are by topic in `repos/`.

mod build_rs;
mod conf;
mod config_files;

pub(super) use build_rs::*;
pub(super) use conf::*;
pub(super) use config_files::*;

use super::*;
use crate::checks::planted::{self, Repo};
use crate::scratch::Scratch;

/// `tauri.conf.json` and `build.rs` of the desktop crate, by their path from the repo root.
const CONF_AT: &str = "crates/desktop/tauri.conf.json";
const BUILD_AT: &str = "crates/desktop/build.rs";

/// A scratch repo whose one member is the desktop crate, holding `files`, each a path from the
/// repo root with its bytes.
fn desktop_repo(case: &str, files: &[(&str, &[u8])]) -> Repo {
    let dir = planted::scratch(&format!("tauri-config-{case}"), files);
    Scratch::create_dir_all(dir.join("crates/desktop")).expect("create the desktop dir");
    Repo::desktop(dir)
}

/// `CONF` with `from` replaced by `to`, once.
fn edited_conf(from: &str, to: &str) -> String {
    assert!(CONF.contains(from), "the fixture contains {from}");
    CONF.replacen(from, to, 1)
}

/// A desktop crate whose `tauri.conf.json` is `CONF` with `from` replaced by `to`, and whose
/// `build.rs` is valid.
fn conf_case(case: &str, from: &str, to: &str) -> Repo {
    let conf = edited_conf(from, to);
    desktop_repo(
        case,
        &[(CONF_AT, conf.as_bytes()), (BUILD_AT, BUILD.as_bytes())],
    )
}

/// A desktop crate whose `build.rs` is `build` and whose `tauri.conf.json` is valid.
fn build_case(case: &str, build: &str) -> Repo {
    desktop_repo(
        case,
        &[(CONF_AT, CONF.as_bytes()), (BUILD_AT, build.as_bytes())],
    )
}

/// A desktop crate whose `build.rs` is valid and whose `tauri.conf.json` is `conf`.
fn raw_conf_case(case: &str, conf: &[u8]) -> Repo {
    desktop_repo(case, &[(CONF_AT, conf), (BUILD_AT, BUILD.as_bytes())])
}

/// The valid desktop crate with a platform config at `path` next to its valid files.
fn platform_case(case: &str, path: &str, text: &str) -> Repo {
    desktop_repo(
        case,
        &[
            (CONF_AT, CONF.as_bytes()),
            (BUILD_AT, BUILD.as_bytes()),
            (path, text.as_bytes()),
        ],
    )
}

pub(super) fn clean_tree() -> Repo {
    desktop_repo(
        "clean-tree",
        &[(CONF_AT, CONF.as_bytes()), (BUILD_AT, BUILD.as_bytes())],
    )
}

pub(super) fn desktop_crate_missing() -> Repo {
    Repo::files::<&str>("tauri-config-desktop-crate-missing", &[])
}

/// The desktop crate with neither tauri.conf.json nor build.rs: the check looks and finds none.
pub(super) fn both_config_files_missing() -> Repo {
    desktop_repo("both-config-files-missing", &[])
}
