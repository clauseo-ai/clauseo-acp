//! Tests of `workspace.rs`: the metadata it reads, the target paths, the workspace root and the
//! package lookup.

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

/// A target root outside the workspace on Windows: drive and all, shown with `/`.
#[cfg(windows)]
#[test]
fn a_windows_target_outside_the_workspace_is_named_with_slashes() {
    let target = Target::new(
        vec!["lib".to_owned()],
        Path::new(r"D:\elsewhere\crate\..\x.rs"),
        Path::new(r"C:\ws"),
    );
    assert_eq!(
        (target.src_path.as_str(), target.outside),
        ("D:/elsewhere/x.rs", true)
    );
    let inside = Target::new(
        vec!["lib".to_owned()],
        Path::new(r"C:\ws\crates\a\src\lib.rs"),
        Path::new(r"C:\ws"),
    );
    assert_eq!(
        (inside.src_path.as_str(), inside.outside),
        ("crates/a/src/lib.rs", false)
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

/// On Unix a backslash is an ordinary file-name character, so a name that holds one keeps it,
/// both outside the workspace and inside it.
#[cfg(unix)]
#[test]
fn a_unix_target_root_keeps_its_backslashes() {
    let target = |path: &str| {
        let target = Target::new(vec!["lib".to_owned()], Path::new(path), Path::new("/ws"));
        (target.src_path, target.outside)
    };
    assert_eq!(
        target(r"/else\where/a\b.rs"),
        (r"/else\where/a\b.rs".to_owned(), true)
    );
    assert_eq!(
        target(r"/ws/cr\ates/lib.rs"),
        (r"cr\ates/lib.rs".to_owned(), false)
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
    crate::scratch::Scratch::write(dir.join("Cargo.toml"), manifest).expect("write the manifest");
    let error = load(&dir).expect_err("cargo metadata fails");
    assert!(error.starts_with("`cargo metadata` failed:\n"), "{error}");
}

/// A worktree that copied another checkout's `target/xtask-run` runs a binary compiled in that
/// checkout: the root must come from what cargo sets as xtask runs, not from `env!`. The run
/// dir is a real folder, so on Windows it has a drive and backslashes.
#[test]
fn the_root_is_the_checkout_xtask_runs_in_not_the_one_it_was_compiled_in() {
    let scratch = Scratch::new("workspace-run-dir");
    let worktree = scratch.join("new/worktree");
    let manifest = "[package]\nname = \"xtask\"\n";
    for dir in ["new/worktree/xtask", "bare/xtask", "other/tool"] {
        Scratch::create_dir_all(scratch.join(dir)).expect("a dir");
    }
    Scratch::write(worktree.join("xtask/Cargo.toml"), manifest).expect("xtask's manifest");
    Scratch::write(scratch.join("other/tool/Cargo.toml"), manifest).expect("another dir");
    let compiled = Path::new("/src/checkout/xtask");
    let run_dir = worktree.join("xtask");
    let run_dir = run_dir.as_os_str();
    let xtask = Some(OsStr::new("xtask"));
    assert_eq!(root_from(Some(run_dir), xtask, compiled), worktree);
    let old = PathBuf::from("/src/checkout");
    assert_eq!(root_from(None, xtask, compiled), old, "unset");
    assert_eq!(
        root_from(Some(OsStr::new("")), xtask, compiled),
        old,
        "empty"
    );
    let desktop = Some(OsStr::new("clauseo-desktop"));
    assert_eq!(
        root_from(Some(run_dir), desktop, compiled),
        old,
        "another package's dir"
    );
    assert_eq!(
        root_from(Some(run_dir), None, compiled),
        old,
        "no package name"
    );
    let bare = scratch.join("bare/xtask");
    assert_eq!(
        root_from(Some(bare.as_os_str()), xtask, compiled),
        old,
        "a dir named xtask without a Cargo.toml"
    );
    let other = scratch.join("other/tool");
    assert_eq!(
        root_from(Some(other.as_os_str()), xtask, compiled),
        old,
        "a dir with a Cargo.toml not named xtask"
    );
    assert_eq!(
        root_from(None, None, Path::new("/")),
        PathBuf::from("."),
        "a dir with no parent"
    );
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
