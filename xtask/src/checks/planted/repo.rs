//! `Repo`, the scratch repo a planted case builds, and the processes builders start (git, cargo),
//! kept here so every planted case starts them the same way.

use crate::checks::DESKTOP_PACKAGE;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;
use crate::workspace::{self, Context, Member, PackageDependencies};
use std::any::Any;
use std::path::Path;

/// A planted case's scratch repo and the context its check runs in. The directory is removed
/// when the `Repo` drops, after what it holds (`holding`).
pub(crate) struct Repo {
    /// Values that must live while the check runs, such as a lock a live run would hold. Fields
    /// drop in order, so these go before the directory.
    held: Vec<Box<dyn Any>>,
    ctx: Context,
    /// Kept so the directory lives, and is removed, with the repo.
    _dir: Scratch,
}

impl Repo {
    /// The repo in `dir`, with `members` as the workspace members and the resolved graph off.
    pub(crate) fn with_members(dir: Scratch, members: Vec<Member>) -> Repo {
        Repo::with_context(dir, |root| Context {
            root: root.to_path_buf(),
            members: Ok(members),
            load_resolved_graph: false,
        })
    }

    /// The repo in `dir`, for a check that reads no workspace member.
    pub(crate) fn new(dir: Scratch) -> Repo {
        Repo::with_members(dir, Vec::new())
    }

    /// The repo in `dir` whose one member is the desktop crate at `crates/desktop`, for a check
    /// that reads only where that crate lies.
    pub(crate) fn desktop(dir: Scratch) -> Repo {
        Repo::with_members(dir, vec![desktop_member()])
    }

    /// The repo `scratch(case, files)` writes, for a check that reads no workspace member.
    pub(crate) fn files<T: AsRef<[u8]>>(case: &str, files: &[(&str, T)]) -> Repo {
        Repo::new(scratch(case, files))
    }

    /// The repo in `dir` with the context `context` makes from its root, such as a failed load.
    pub(crate) fn with_context(dir: Scratch, context: impl FnOnce(&Path) -> Context) -> Repo {
        let ctx = context(&dir);
        assert_eq!(
            ctx.root,
            dir.to_path_buf(),
            "the context's root is the scratch repo"
        );
        Repo {
            held: Vec::new(),
            ctx,
            _dir: dir,
        }
    }

    /// The cargo workspace in `dir`, with the members the real `cargo metadata` lists, and the
    /// resolved graph off.
    pub(crate) fn cargo(dir: Scratch) -> Repo {
        let members = workspace::load(&dir).expect("cargo metadata runs");
        Repo::with_members(dir, members)
    }

    /// `cargo`, with the resolved graph on, as `cargo xtask check` runs: the check walks it with
    /// `cargo tree --locked --offline`, so cargo first writes the lockfile, offline.
    pub(crate) fn resolved(dir: Scratch) -> Repo {
        lock(&dir);
        let mut repo = Repo::cargo(dir);
        repo.ctx.load_resolved_graph = true;
        repo
    }

    /// `resolved`, for the workspace in the folder `repo` of `dir`, so cargo also reads the
    /// config files of `dir` (cargo reads `.cargo/config.toml` in every parent folder), while
    /// the check reads only the workspace's own.
    pub(crate) fn resolved_in(dir: Scratch, repo: &str) -> Repo {
        let root = dir.join(repo);
        lock(&root);
        let members = workspace::load(&root).expect("cargo metadata runs");
        Repo {
            held: Vec::new(),
            ctx: Context {
                root,
                members: Ok(members),
                load_resolved_graph: true,
            },
            _dir: dir,
        }
    }

    /// The repo, holding `value` until the check has run.
    pub(crate) fn holding(mut self, value: impl Any) -> Repo {
        self.held.push(Box::new(value));
        self
    }

    /// The context the check runs in.
    pub(crate) fn ctx(&self) -> &Context {
        &self.ctx
    }
}

/// A scratch directory named `planted-{case}`, with each `(path, bytes)` of `files` written into
/// it, its parent directories first. `path` is relative to the directory, with `/` between parts.
/// Start `case` with the check, e.g. `budgets-negative-limit`, so a directory a killed run left
/// says which case made it.
pub(crate) fn scratch<T: AsRef<[u8]>>(case: &str, files: &[(&str, T)]) -> Scratch {
    let dir = Scratch::new(&format!("planted-{case}"));
    for (path, bytes) in files {
        let full = dir.join(path);
        if let Some(parent) = full.parent() {
            Scratch::create_dir_all(parent).expect("create the file's parent directories");
        }
        Scratch::write(&full, bytes).expect("write the case's file");
    }
    dir
}

/// Writes the lockfile of the workspace at `root` with `cargo generate-lockfile --offline`.
fn lock(root: &Path) {
    let lock = Cmd::new(
        Program::Cargo,
        ["generate-lockfile", "--offline"],
        process::TEST_TIMEOUT,
    );
    let quiet = process::run_quiet(&lock, root).expect("run cargo generate-lockfile");
    assert!(
        quiet.success(),
        "cargo generate-lockfile failed: {}",
        quiet.stderr
    );
}

/// The desktop crate as a member at `crates/desktop`, with no target or dependency.
pub(crate) fn desktop_member() -> Member {
    Member {
        name: DESKTOP_PACKAGE.to_owned(),
        dir: "crates/desktop".to_owned(),
        path_deps: Vec::new(),
        targets: Vec::new(),
        dependencies: PackageDependencies::default(),
    }
}

/// Runs `git <args>` in `dir` and asserts that it succeeded.
pub(crate) fn git(dir: &Path, args: &[&str]) {
    let quiet = process::run_quiet(&Cmd::new(Program::Git, args, process::TEST_TIMEOUT), dir)
        .expect("run git");
    assert!(quiet.success(), "git {args:?} failed: {}", quiet.stderr);
}
