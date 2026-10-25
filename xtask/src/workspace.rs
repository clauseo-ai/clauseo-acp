//! Workspace layout read from `cargo metadata`, and the context handed to static checks.

use crate::process::{self, Cmd, Program, Status};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// A workspace member package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub name: String,
    /// Directory relative to the workspace root, `/`-separated.
    pub dir: String,
    /// Directories (relative to the workspace root) of this package's path dependencies.
    pub path_deps: Vec<String>,
    /// The package's targets, as `cargo metadata` lists them.
    pub targets: Vec<Target>,
    /// The package's dependencies and `[features]` table, as `cargo metadata` lists them.
    pub dependencies: PackageDependencies,
}

/// One target of a member package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The target's kinds: `lib`, `bin`, `test`, `example`, `bench`, `custom-build` and others.
    pub kinds: Vec<String>,
    /// The target's root file with its `.` and `..` components resolved, relative to the
    /// workspace root, or absolute when `outside`; `/`-separated on every OS either way.
    pub src_path: String,
    /// True when the root file lies outside the workspace root, where cargo still builds it.
    pub outside: bool,
}

impl Target {
    /// The target of kinds `kinds` whose root `cargo metadata` gives as `src_path`, an absolute
    /// path that may hold `..`, in the workspace at `base`.
    pub(crate) fn new(kinds: Vec<String>, src_path: &Path, base: &Path) -> Target {
        match relative(src_path, base) {
            Some(relative) => Target {
                kinds,
                src_path: relative,
                outside: false,
            },
            None => Target {
                kinds,
                src_path: lexical(src_path)
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
                outside: true,
            },
        }
    }
}

/// Passed to every static check.
pub(crate) struct Context {
    pub root: PathBuf,
    /// The workspace members from one `cargo metadata --no-deps` run, or the error of that run.
    /// A check that reads members stops on the error (`checks::members`), since an empty list
    /// would make it report fixes for a workspace that is not there.
    pub members: Result<Vec<Member>, String>,
    /// Whether checks may load the dependency graph cargo resolved (`cargo tree --locked
    /// --offline` per crate, for plugins and dependencies: `checks::graph`). That graph
    /// needs every target's crates downloaded, so tests of the real repo, which use no network,
    /// leave it off; `cargo xtask check` turns it on.
    pub load_resolved_graph: bool,
}

/// The parts of `cargo metadata --format-version 1 --no-deps` that xtask reads.
#[derive(Deserialize)]
struct Metadata {
    workspace_root: PathBuf,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<CargoDependency>,
    features: BTreeMap<String, Vec<String>>,
    targets: Vec<PackageTarget>,
}

/// One entry of a package's `targets` in `cargo metadata`.
#[derive(Deserialize)]
struct PackageTarget {
    kind: Vec<String>,
    src_path: PathBuf,
}

/// One dependency of a package as `cargo metadata` reports it, with workspace inheritance
/// resolved: `name` is the real package, `rename` the key it goes by in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CargoDependency {
    pub name: String,
    pub rename: Option<String>,
    /// `None` for a normal dependency, else `"dev"` or `"build"`.
    pub kind: Option<String>,
    /// The `cfg(...)` or target triple of a `[target.*.dependencies]` entry.
    pub target: Option<String>,
    pub optional: bool,
    /// The features the entry enables, its own and its workspace entry's merged.
    pub features: Vec<String>,
    /// The directory of a path dependency, else `None`.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The version requirement as cargo writes it, such as `^1.2` or `>=1, <2`.
    #[serde(default)]
    pub req: String,
    /// Where the package comes from, such as `git+https://example.com/s?branch=main` for a git
    /// dependency or `registry+https://github.com/rust-lang/crates.io-index`. `None` for a path
    /// dependency.
    #[serde(default)]
    pub source: Option<String>,
}

impl CargoDependency {
    /// The key the dependency goes by in the manifest and in `[features]`: its rename, else its
    /// name.
    pub(crate) fn key(&self) -> &str {
        self.rename.as_deref().unwrap_or(&self.name)
    }

    /// True for a `[dependencies]` or `[target.*.dependencies]` entry, false for dev and build.
    pub(crate) fn is_normal(&self) -> bool {
        matches!(self.kind.as_deref(), None | Some("normal"))
    }
}

/// One package's dependencies and its `[features]` table, from `cargo metadata`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PackageDependencies {
    pub dependencies: Vec<CargoDependency>,
    pub features: BTreeMap<String, Vec<String>>,
}

/// The workspace root: the parent of this crate's directory (`xtask/`) in the checkout xtask runs
/// in, which `root_from` reads from the variables `cargo run` (and so the `cargo xtask` alias)
/// sets for the program it runs.
pub(crate) fn workspace_root() -> PathBuf {
    root_from(
        std::env::var_os("CARGO_MANIFEST_DIR").as_deref(),
        std::env::var_os("CARGO_PKG_NAME").as_deref(),
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
}

/// The parent of xtask's directory. `run_dir` and `run_package` are `CARGO_MANIFEST_DIR` and
/// `CARGO_PKG_NAME` as xtask runs: cargo sets both for the program `cargo run` starts (cargo 1.99:
/// a moved crate whose sources are older than its binary prints the new dir at run time and the
/// old one through `env!`). `run_dir` is used when `run_package` is this package's own name and
/// `run_dir` looks like xtask's directory (`is_xtask_dir`). That guard rejects another package's
/// dir, a dir not named `xtask`, or one without a `Cargo.toml`, so such a dir that cargo left in
/// the environment is not taken for xtask's. Otherwise `compiled_dir`, the dir xtask was compiled
/// in, which a checkout that copied another checkout's `target/xtask-run` would get wrong: cargo
/// sees the copied binary as up to date, since it ignores the variables it sets itself when it
/// checks for changes.
pub(crate) fn root_from(
    run_dir: Option<&OsStr>,
    run_package: Option<&OsStr>,
    compiled_dir: &Path,
) -> PathBuf {
    let own = run_package == Some(OsStr::new(env!("CARGO_PKG_NAME")));
    let dir = match run_dir.map(Path::new) {
        Some(dir) if own && is_xtask_dir(dir) => dir,
        _ => compiled_dir,
    };
    dir.parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Whether `dir` looks like xtask's directory: its last part is `xtask` and it holds a
/// `Cargo.toml`. An empty `dir` has no last part.
fn is_xtask_dir(dir: &Path) -> bool {
    dir.file_name() == Some(OsStr::new("xtask")) && dir.join("Cargo.toml").is_file()
}

/// Runs `cargo metadata` in `root` once and reads the workspace members from its output.
pub(crate) fn load(root: &Path) -> Result<Vec<Member>, String> {
    parse_metadata(&run_metadata(root)?)
}

/// How long `cargo metadata --no-deps` may run: it reads the workspace manifests and resolves
/// nothing, which takes well under a second, so 2 min means it hangs (on a lock, say).
const METADATA_TIMEOUT: Duration = Duration::from_mins(2);

/// The stdout of `cargo metadata --format-version 1 --no-deps` run in `root`. A run past its limit
/// says so in place of the start error.
fn run_metadata(root: &Path) -> Result<String, String> {
    let cmd = Cmd::new(
        Program::Cargo,
        ["metadata", "--format-version", "1", "--no-deps"],
        METADATA_TIMEOUT,
    );
    let quiet = process::run_quiet(&cmd, root)
        .map_err(|e| format!("could not run `cargo metadata`: {e}"))?;
    if quiet.status == Status::TimedOut {
        return Err(process::timeout_message(&cmd));
    }
    if !quiet.success() {
        return Err(format!("`cargo metadata` failed:\n{}", quiet.stderr.trim()));
    }
    Ok(quiet.stdout)
}

/// Reads `cargo metadata --no-deps` JSON. Paths are made relative to `workspace_root`.
pub(crate) fn parse_metadata(json: &str) -> Result<Vec<Member>, String> {
    let metadata: Metadata =
        serde_json::from_str(json).map_err(|e| format!("bad `cargo metadata` JSON: {e}"))?;
    let base = metadata.workspace_root.as_path();

    let mut members = Vec::new();
    for package in metadata.packages {
        let dir = package
            .manifest_path
            .parent()
            .and_then(|p| relative(p, base))
            .ok_or_else(|| format!("package {} is outside the workspace", package.name))?;
        let path_deps = package
            .dependencies
            .iter()
            .filter_map(|dep| dep.path.as_deref())
            .filter_map(|p| relative(p, base))
            .collect();
        let targets = package
            .targets
            .iter()
            .map(|target| Target::new(target.kind.clone(), &target.src_path, base))
            .collect();
        members.push(Member {
            name: package.name,
            dir,
            path_deps,
            targets,
            dependencies: PackageDependencies {
                dependencies: package.dependencies,
                features: package.features,
            },
        });
    }
    members.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(members)
}

/// `path` relative to `base`, `/`-separated, after `lexical` resolves both; `None` when
/// `path` is outside `base`.
fn relative(path: &Path, base: &Path) -> Option<String> {
    let path = lexical(path);
    let base = lexical(base);
    let rest = path.strip_prefix(&base).ok()?;
    Some(
        rest.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// `path` with its `.` components dropped and each `..` removing the component before it,
/// without reading the disk. A `..` at the root or at the start of a relative path stays.
fn lexical(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::ParentDir | Component::CurDir) | None => out.push(component),
            },
            other => out.push(other),
        }
    }
    out.iter().collect()
}

/// True when the repo-relative `file` lies inside the repo-relative directory `dir`.
pub(crate) fn is_under(file: &str, dir: &str) -> bool {
    dir.is_empty() || file == dir || file.starts_with(&format!("{dir}/"))
}

/// The member whose directory is the longest prefix of `file`.
pub(crate) fn package_for<'a>(file: &str, members: &'a [Member]) -> Option<&'a Member> {
    members
        .iter()
        .filter(|m| is_under(file, &m.dir))
        .max_by_key(|m| m.dir.len())
}

#[cfg(test)]
mod tests;
