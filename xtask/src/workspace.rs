//! Workspace layout read from `cargo metadata`, and the context handed to static checks.

use crate::process::{self, Cmd, Program};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

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
    /// workspace root and `/`-separated, or absolute when `outside`.
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
                src_path: lexical(src_path).to_string_lossy().into_owned(),
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

/// The workspace root: the parent of this crate's directory (`xtask/`).
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Runs `cargo metadata` in `root` once and reads the workspace members from its output.
pub(crate) fn load(root: &Path) -> Result<Vec<Member>, String> {
    parse_metadata(&run_metadata(root)?)
}

/// The stdout of `cargo metadata --format-version 1 --no-deps` run in `root`.
fn run_metadata(root: &Path) -> Result<String, String> {
    let cmd = Cmd::new(
        Program::Cargo,
        ["metadata", "--format-version", "1", "--no-deps"],
    );
    let quiet = process::run_quiet(&cmd, root)
        .map_err(|e| format!("could not run `cargo metadata`: {e}"))?;
    if !quiet.success {
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
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    const SAMPLE: &str = r#"{
        "workspace_root": "/ws",
        "packages": [
            {"name": "b", "manifest_path": "/ws/crates/b/Cargo.toml",
             "features": {"x": ["t/devtools"]},
             "dependencies": [
                {"name": "serde", "rename": null, "kind": null, "target": null,
                 "optional": false, "features": [], "path": null, "req": "^1"},
                {"name": "a", "rename": null, "kind": null, "target": null,
                 "optional": false, "features": [], "path": "/ws/crates/a", "req": "*"},
                {"name": "tauri", "rename": "t", "kind": null, "target": "cfg(unix)",
                 "optional": false, "features": ["devtools"], "req": "^2"}],
             "targets": [{"kind": ["lib"], "src_path": "/ws/crates/b/src/lib.rs"},
                         {"kind": ["test"], "src_path": "/ws/crates/b/tests/t.rs"}]},
            {"name": "a", "manifest_path": "/ws/crates/a/Cargo.toml",
             "features": {},
             "dependencies": [{"name": "outside", "rename": null, "kind": "dev", "target": null,
                               "optional": false, "features": [], "path": "/elsewhere/x"}],
             "targets": []}
        ]
    }"#;

    #[test]
    fn parses_members_with_relative_dirs_and_path_deps() {
        let members = parse_metadata(SAMPLE).expect("sample parses");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "a");
        assert_eq!(members[0].dir, "crates/a");
        assert_eq!(members[0].path_deps, Vec::<String>::new());
        assert_eq!(members[1].name, "b");
        assert_eq!(members[1].dir, "crates/b");
        assert_eq!(members[1].path_deps, vec!["crates/a".to_owned()]);
        assert_eq!(
            members[1].targets,
            [
                Target {
                    kinds: vec!["lib".to_owned()],
                    src_path: "crates/b/src/lib.rs".to_owned(),
                    outside: false,
                },
                Target {
                    kinds: vec!["test".to_owned()],
                    src_path: "crates/b/tests/t.rs".to_owned(),
                    outside: false,
                },
            ]
        );
        let json = SAMPLE.replace("/ws/crates/b/tests/t.rs", "/elsewhere/t.rs");
        let members = parse_metadata(&json).expect("a target outside the workspace parses");
        let outside = &members[1].targets[1];
        assert_eq!(
            (outside.src_path.as_str(), outside.outside),
            ("/elsewhere/t.rs", true)
        );
    }

    /// cargo metadata keeps a manifest's `..`; the target is placed by the path it resolves to.
    #[test]
    fn a_target_root_with_dot_dot_is_placed_where_it_resolves() {
        let target = |path: &str| {
            let target = Target::new(vec!["lib".to_owned()], Path::new(path), Path::new("/ws"));
            (target.src_path, target.outside)
        };
        assert_eq!(
            target("/ws/crates/a/../../../x.rs"),
            ("/x.rs".to_owned(), true)
        );
        assert_eq!(
            target("/ws/crates/a/../shared/./lib.rs"),
            ("crates/shared/lib.rs".to_owned(), false)
        );
        assert_eq!(
            target("/ws/crates/../../ws/crates/a/lib.rs"),
            ("crates/a/lib.rs".to_owned(), false)
        );
    }

    #[test]
    fn parses_every_member_s_dependencies_and_features() {
        let members = parse_metadata(SAMPLE).expect("sample parses");
        let deps = &members[1].dependencies;
        assert_eq!(deps.dependencies.len(), 3);
        let tauri = &deps.dependencies[2];
        assert_eq!((tauri.name.as_str(), tauri.key()), ("tauri", "t"));
        assert!(tauri.is_normal());
        assert_eq!(tauri.target.as_deref(), Some("cfg(unix)"));
        assert_eq!(tauri.features, ["devtools"]);
        assert_eq!(deps.features["x"], ["t/devtools"]);
        assert!(!members[0].dependencies.dependencies[0].is_normal());
        assert!(parse_metadata("{").is_err());
    }

    #[test]
    fn a_failed_cargo_metadata_run_is_an_error_of_the_load() {
        let dir = Scratch::new("workspace-load-fails");
        let manifest = "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                        [dependencies]\nfoo = \"not a req\"\n";
        crate::scratch::Scratch::write(dir.join("Cargo.toml"), manifest)
            .expect("write the manifest");
        let error = load(&dir).expect_err("cargo metadata fails");
        assert!(error.starts_with("`cargo metadata` failed:\n"), "{error}");
    }

    #[test]
    fn package_for_picks_longest_directory_prefix() {
        let members = vec![
            Member {
                name: "outer".into(),
                dir: "crates".into(),
                path_deps: vec![],
                targets: vec![],
                dependencies: PackageDependencies::default(),
            },
            Member {
                name: "inner".into(),
                dir: "crates/inner".into(),
                path_deps: vec![],
                targets: vec![],
                dependencies: PackageDependencies::default(),
            },
        ];
        assert_eq!(
            package_for("crates/inner/src/lib.rs", &members).map(|m| m.name.as_str()),
            Some("inner")
        );
        assert_eq!(
            package_for("crates/other.rs", &members).map(|m| m.name.as_str()),
            Some("outer")
        );
        assert!(package_for("README.md", &members).is_none());
        assert!(!is_under("crates/innerx/a.rs", "crates/inner"));
    }
}
