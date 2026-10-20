//! Workspace layout read from `cargo metadata`, and the context handed to static checks.

use crate::process::{self, Cmd, Program};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
}

/// One target of a member package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Target {
    /// The target's kinds: `lib`, `bin`, `test`, `example`, `bench`, `custom-build` and others.
    pub kinds: Vec<String>,
    /// The target's root file, relative to the workspace root and `/`-separated, or as
    /// `cargo metadata` gives it when `outside`.
    pub src_path: String,
    /// True when the root file lies outside the workspace root, where cargo still builds it.
    pub outside: bool,
}

/// The workspace as one `cargo metadata --no-deps` run reports it.
#[derive(Default)]
pub(crate) struct Workspace {
    pub members: Vec<Member>,
    /// The dependencies and `[features]` of every member, by package name.
    pub dependencies: BTreeMap<String, PackageDependencies>,
}

/// Passed to every static check.
pub(crate) struct Context {
    pub root: PathBuf,
    pub members: Vec<Member>,
    /// The dependencies and `[features]` of every member, by package name, from the same
    /// `cargo metadata` run as `members`. Empty when that run failed, which `cargo xtask check`
    /// reports as a note before any check runs.
    pub dependencies: BTreeMap<String, PackageDependencies>,
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
    dependencies: Vec<Dependency>,
    targets: Vec<PackageTarget>,
}

/// One entry of a package's `targets` in `cargo metadata`.
#[derive(Deserialize)]
struct PackageTarget {
    kind: Vec<String>,
    src_path: PathBuf,
}

#[derive(Deserialize)]
struct Dependency {
    /// Set for path dependencies.
    path: Option<PathBuf>,
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct PackageDependencies {
    pub dependencies: Vec<CargoDependency>,
    pub features: BTreeMap<String, Vec<String>>,
}

/// The parts of a package that `parse_workspace_dependencies` reads.
#[derive(Deserialize)]
struct PackageWithDependencies {
    name: String,
    #[serde(flatten)]
    dependencies: PackageDependencies,
}

/// The packages of `cargo metadata` JSON, for `parse_workspace_dependencies`.
#[derive(Deserialize)]
struct DependencyMetadata {
    packages: Vec<PackageWithDependencies>,
}

/// The workspace root: the parent of this crate's directory (`xtask/`).
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Runs `cargo metadata` in `root` once and reads the workspace from its output.
pub(crate) fn load(root: &Path) -> Result<Workspace, String> {
    let json = run_metadata(root)?;
    Ok(Workspace {
        members: parse_metadata(&json)?,
        dependencies: parse_workspace_dependencies(&json)?,
    })
}

/// Runs `cargo metadata` in `root` and returns the workspace members.
pub(crate) fn load_members(root: &Path) -> Result<Vec<Member>, String> {
    load(root).map(|workspace| workspace.members)
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
        return Err(format!("`cargo metadata` failed: {}", quiet.stderr.trim()));
    }
    Ok(quiet.stdout)
}

/// Reads the dependencies and `[features]` of every package in `cargo metadata` JSON, by name.
pub(crate) fn parse_workspace_dependencies(
    json: &str,
) -> Result<BTreeMap<String, PackageDependencies>, String> {
    let metadata: DependencyMetadata =
        serde_json::from_str(json).map_err(|e| format!("bad `cargo metadata` JSON: {e}"))?;
    Ok(metadata
        .packages
        .into_iter()
        .map(|package| (package.name, package.dependencies))
        .collect())
}

/// Reads `cargo metadata --no-deps` JSON. Paths are made relative to `workspace_root`.
pub(crate) fn parse_metadata(json: &str) -> Result<Vec<Member>, String> {
    let metadata: Metadata =
        serde_json::from_str(json).map_err(|e| format!("bad `cargo metadata` JSON: {e}"))?;
    let base = metadata.workspace_root.as_path();

    let mut members = Vec::new();
    for package in &metadata.packages {
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
            .map(|target| match relative(&target.src_path, base) {
                Some(src_path) => Target {
                    kinds: target.kind.clone(),
                    src_path,
                    outside: false,
                },
                None => Target {
                    kinds: target.kind.clone(),
                    src_path: target.src_path.to_string_lossy().into_owned(),
                    outside: true,
                },
            })
            .collect();
        members.push(Member {
            name: package.name.clone(),
            dir,
            path_deps,
            targets,
        });
    }
    members.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(members)
}

/// `path` relative to `base`, `/`-separated; `None` when `path` is outside `base`.
fn relative(path: &Path, base: &Path) -> Option<String> {
    let rest = path.strip_prefix(base).ok()?;
    Some(
        rest.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
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
    use std::fs;

    const SAMPLE: &str = r#"{
        "workspace_root": "/ws",
        "packages": [
            {"name": "b", "manifest_path": "/ws/crates/b/Cargo.toml",
             "dependencies": [{"name": "serde", "path": null},
                              {"name": "a", "path": "/ws/crates/a"}],
             "targets": [{"kind": ["lib"], "src_path": "/ws/crates/b/src/lib.rs"},
                         {"kind": ["test"], "src_path": "/ws/crates/b/tests/t.rs"}]},
            {"name": "a", "manifest_path": "/ws/crates/a/Cargo.toml",
             "dependencies": [{"name": "outside", "path": "/elsewhere/x"}],
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

    #[test]
    fn parses_every_package_s_dependencies_and_features() {
        let json = r#"{"packages": [
            {"name": "other", "dependencies": [], "features": {}},
            {"name": "app", "features": {"x": ["t/devtools"]}, "dependencies": [
                {"name": "tauri", "rename": "t", "kind": null, "target": "cfg(unix)",
                 "optional": false, "features": ["devtools"], "req": "^2"}]}
        ]}"#;
        let packages = parse_workspace_dependencies(json).expect("sample parses");
        assert_eq!(packages.keys().collect::<Vec<_>>(), ["app", "other"]);
        let deps = &packages["app"];
        assert_eq!(deps.dependencies.len(), 1);
        let tauri = &deps.dependencies[0];
        assert_eq!((tauri.name.as_str(), tauri.key()), ("tauri", "t"));
        assert!(tauri.is_normal());
        assert_eq!(tauri.target.as_deref(), Some("cfg(unix)"));
        assert_eq!(tauri.features, ["devtools"]);
        assert_eq!(deps.features["x"], ["t/devtools"]);
        assert!(parse_workspace_dependencies("{").is_err());
    }

    #[test]
    fn a_failed_cargo_metadata_run_is_an_error_of_the_load() {
        let dir = Scratch::new("workspace-load-fails");
        let manifest = "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
                        [dependencies]\nfoo = \"not a req\"\n";
        fs::write(dir.join("Cargo.toml"), manifest).expect("write the manifest");
        let error = load(&dir).err().expect("cargo metadata fails");
        assert!(error.starts_with("`cargo metadata` failed: "), "{error}");
    }

    #[test]
    fn package_for_picks_longest_directory_prefix() {
        let members = vec![
            Member {
                name: "outer".into(),
                dir: "crates".into(),
                path_deps: vec![],
                targets: vec![],
            },
            Member {
                name: "inner".into(),
                dir: "crates/inner".into(),
                path_deps: vec![],
                targets: vec![],
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
