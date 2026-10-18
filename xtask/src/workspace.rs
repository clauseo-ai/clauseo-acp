//! Workspace layout read from `cargo metadata`, and the context handed to static checks.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A workspace member package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// Directory relative to the workspace root, `/`-separated.
    pub dir: String,
    /// Directories (relative to the workspace root) of this package's path dependencies.
    pub path_deps: Vec<String>,
}

/// Passed to every static check.
pub struct Context {
    pub root: PathBuf,
    pub members: Vec<Member>,
}

/// The workspace root: the parent of this crate's directory (`xtask/`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Runs `cargo metadata` in `root` and returns the workspace members.
pub fn load_members(root: &Path) -> Result<Vec<Member>, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("could not run `cargo metadata`: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "`cargo metadata` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_metadata(&String::from_utf8_lossy(&output.stdout))
}

/// Reads `cargo metadata --no-deps` JSON. Paths are made relative to `workspace_root`.
pub fn parse_metadata(json: &str) -> Result<Vec<Member>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad `cargo metadata` JSON: {e}"))?;
    let base = value
        .get("workspace_root")
        .and_then(|v| v.as_str())
        .ok_or("`cargo metadata` has no workspace_root")?;
    let base = Path::new(base);
    let packages = value
        .get("packages")
        .and_then(|v| v.as_array())
        .ok_or("`cargo metadata` has no packages")?;

    let mut members = Vec::new();
    for pkg in packages {
        let name = pkg
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("`cargo metadata` package has no name")?
            .to_string();
        let manifest = pkg
            .get("manifest_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("package {name} has no manifest_path"))?;
        let dir = Path::new(manifest)
            .parent()
            .and_then(|p| relative(p, base))
            .ok_or_else(|| format!("package {name} is outside the workspace"))?;
        let path_deps = pkg
            .get("dependencies")
            .and_then(|v| v.as_array())
            .map(|deps| {
                deps.iter()
                    .filter_map(|d| d.get("path").and_then(|p| p.as_str()))
                    .filter_map(|p| relative(Path::new(p), base))
                    .collect()
            })
            .unwrap_or_default();
        members.push(Member {
            name,
            dir,
            path_deps,
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
pub fn is_under(file: &str, dir: &str) -> bool {
    dir.is_empty() || file == dir || file.starts_with(&format!("{dir}/"))
}

/// The member whose directory is the longest prefix of `file`.
pub fn package_for<'a>(file: &str, members: &'a [Member]) -> Option<&'a Member> {
    members
        .iter()
        .filter(|m| is_under(file, &m.dir))
        .max_by_key(|m| m.dir.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "workspace_root": "/ws",
        "packages": [
            {"name": "b", "manifest_path": "/ws/crates/b/Cargo.toml",
             "dependencies": [{"name": "serde", "path": null},
                              {"name": "a", "path": "/ws/crates/a"}]},
            {"name": "a", "manifest_path": "/ws/crates/a/Cargo.toml",
             "dependencies": [{"name": "outside", "path": "/elsewhere/x"}]}
        ]
    }"#;

    #[test]
    fn parses_members_with_relative_dirs_and_path_deps() {
        let members = parse_metadata(SAMPLE).expect("sample parses");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "a");
        assert_eq!(members[0].dir, "crates/a");
        assert!(members[0].path_deps.is_empty());
        assert_eq!(members[1].name, "b");
        assert_eq!(members[1].dir, "crates/b");
        assert_eq!(members[1].path_deps, vec!["crates/a".to_string()]);
    }

    #[test]
    fn package_for_picks_longest_directory_prefix() {
        let members = vec![
            Member {
                name: "outer".into(),
                dir: "crates".into(),
                path_deps: vec![],
            },
            Member {
                name: "inner".into(),
                dir: "crates/inner".into(),
                path_deps: vec![],
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
