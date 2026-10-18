//! Static checks: Rust code that inspects the repo instead of running a tool.
//!
//! One file per check in `src/checks/`, registered in `ALL` below. `steps::plan` runs every entry
//! after the tool steps and before nextest.

mod budgets;
mod capabilities;
mod json_text;
mod plugins;
mod tauri_config;

use crate::violation::Violation;
use crate::workspace::Context;
use std::fs;
use std::io;
use std::path::Path;

/// A check written in Rust. It fails iff it returns violations.
pub(crate) struct StaticCheck {
    pub name: &'static str,
    pub run: fn(&Context) -> Vec<Violation>,
}

/// Every static check, in run order.
pub(crate) const ALL: &[StaticCheck] = &[
    budgets::CHECK,
    capabilities::CHECK,
    plugins::CHECK,
    tauri_config::CHECK,
];

/// The package name of the desktop crate. The checks find its directory through `Context::members`,
/// so a moved crate is still checked and a missing one fails instead of passing quietly.
pub(crate) const DESKTOP_PACKAGE: &str = "clauseo-desktop";

/// The repo-relative directory of the desktop crate. When it is not a workspace member, the one
/// violation at `Cargo.toml:1` that every desktop check reports.
pub(crate) fn desktop_dir(ctx: &Context) -> Result<&str, Violation> {
    ctx.members
        .iter()
        .find(|member| member.name == DESKTOP_PACKAGE)
        .map(|member| member.dir.as_str())
        .ok_or_else(|| {
            violation(
                "Cargo.toml",
                1,
                format!("the desktop crate (package `{DESKTOP_PACKAGE}`) is not a workspace member"),
                "restore crates/desktop or update xtask/src/checks: these checks guard its capabilities, plugins and Tauri config",
            )
        })
}

/// `dir/name`, or `name` when `dir` is the repo root.
pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// Reads `root/rel` as text. `Ok(None)` when the file does not exist; any other read error is a
/// violation at line 1.
pub(crate) fn read_repo_file(root: &Path, rel: &str) -> Result<Option<String>, Violation> {
    match fs::read_to_string(root.join(rel)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(unreadable(rel, &error)),
    }
}

/// The violation for `path`, which exists but cannot be read as UTF-8 text.
pub(crate) fn unreadable(path: &str, error: &io::Error) -> Violation {
    Violation {
        path: path.to_owned(),
        line: 1,
        message: format!("could not read `{path}`: {error}"),
        fix: "make the file readable as UTF-8 text".to_owned(),
    }
}

/// `path` relative to `root`, with `/` separators.
pub(crate) fn repo_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A violation at `path:line`.
pub(crate) fn violation(
    path: &str,
    line: u32,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation {
        path: path.to_owned(),
        line,
        message: message.into(),
        fix: fix.into(),
    }
}

/// The 1-based line number of the line at `index`, counting from 0 as `str::lines` does.
pub(crate) fn line_at(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_missing_file_reads_as_none_and_a_present_one_as_its_text() {
        let dir = Scratch::new("checks-read");
        assert_eq!(read_repo_file(&dir, "a.txt"), Ok(None));
        fs::write(dir.join("a.txt"), "hello").expect("write the file");
        assert_eq!(read_repo_file(&dir, "a.txt"), Ok(Some("hello".to_owned())));
    }

    #[test]
    fn a_path_is_repo_relative_with_slashes() {
        let root = Path::new("/ws");
        assert_eq!(
            repo_path(root, Path::new("/ws/crates/desktop/x.json")),
            "crates/desktop/x.json"
        );
    }

    /// A context whose members include the desktop crate at `crates/desktop`.
    pub(crate) fn desktop_context(root: &Path) -> Context {
        Context {
            root: root.to_path_buf(),
            members: vec![crate::workspace::Member {
                name: DESKTOP_PACKAGE.to_owned(),
                dir: "crates/desktop".to_owned(),
                path_deps: Vec::new(),
            }],
        }
    }

    #[test]
    fn the_desktop_dir_comes_from_the_member_with_its_package_name() {
        let mut ctx = desktop_context(Path::new("/ws"));
        ctx.members[0].dir = "apps/shell".to_owned();
        assert_eq!(desktop_dir(&ctx), Ok("apps/shell"));
    }

    #[test]
    fn a_missing_desktop_member_is_one_violation_at_the_root_manifest() {
        let ctx = Context {
            root: Path::new("/ws").to_path_buf(),
            members: Vec::new(),
        };
        assert_eq!(
            desktop_dir(&ctx),
            Err(violation(
                "Cargo.toml",
                1,
                "the desktop crate (package `clauseo-desktop`) is not a workspace member",
                "restore crates/desktop or update xtask/src/checks: these checks guard its capabilities, plugins and Tauri config",
            ))
        );
    }

    #[test]
    fn the_real_repo_passes_every_static_check() {
        let root = crate::workspace::workspace_root();
        let ctx = Context {
            members: crate::workspace::load_members(&root).expect("cargo metadata runs"),
            root,
        };
        for check in ALL {
            assert_eq!((check.name, (check.run)(&ctx)), (check.name, Vec::new()));
        }
    }

    #[test]
    fn line_numbers_start_at_one() {
        assert_eq!(line_at(0), 1);
        assert_eq!(line_at(4), 5);
    }
}
