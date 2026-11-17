//! The Codex launch never lists Codex's home (`CODEX_HOME`) or the data folder as writable, on
//! either OS's policy: built through the functions the launch uses (`profile::codex_keys`,
//! [`keys`], `codex::config_on`, and the session's `additionalDirectories`, which
//! `actor::turn::profile_loads::codex_directories` makes the run folder and the notes).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clauseo_settings::Locations;
use serde_json::{Map, Value};

use super::{Session, keys, list};
use crate::agent::loads::browser::Switch;
use crate::agent::loads::profile::{Folders, codex_keys};
use crate::write;

/// A data folder laid out for one Codex launch: its run folder (with `work`), this matter's
/// notes, another session's run folder and Codex's home with its `config.toml`.
struct Launch {
    root: PathBuf,
    data: PathBuf,
    locations: Locations,
    run_root: PathBuf,
    notes: PathBuf,
    home: PathBuf,
}

impl Launch {
    fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("clauseo-codex-{test}-{}", std::process::id()));
        write::remove_dir_all(&root).unwrap();
        let data = root.join("data");
        let locations = Locations::under_root(&data);
        let run_root = data.join("run").join("t-2");
        let notes = data.join("agent-state").join("m-1").join("notes");
        for folder in [
            run_root.join("work"),
            notes.clone(),
            data.join("run").join("t-1"),
        ] {
            write::create_dir_all(&folder).unwrap();
        }
        let home = locations.agent_home("codex-acp");
        crate::agent::codex::make_home(&data, &home).unwrap();
        crate::agent::codex::write_home_config(&home).unwrap();
        Self {
            root,
            data,
            locations,
            run_root,
            notes,
            home,
        }
    }

    /// The launch's `CODEX_CONFIG` for Windows (`windows`) or macOS and Linux.
    fn config(&self, windows: bool) -> Map<String, Value> {
        let work = self.run_root.join("work");
        let folders = Folders::of(&work, &self.notes).unwrap();
        let mut hook = Map::new();
        hook.insert(
            "developer_instructions".to_owned(),
            Value::from("Work in the matter."),
        );
        let web_search = Switch {
            on: true,
            locked: false,
        };
        let mut keys_now = codex_keys(hook, web_search, folders).unwrap();
        let listed = list(&self.data, &self.run_root, &self.notes);
        let session = Session {
            run_root: &self.run_root,
            notes: &self.notes,
            codex_home: &self.home,
            listed: &listed,
            socket_dir: None,
        };
        keys_now.extend(keys(&self.locations, session, windows));
        let program = self.root.join("app").join("clauseo");
        let hooks = match crate::codex_hook_config_with_home(&program, None, Some(&self.home)) {
            Some(Value::Object(hooks)) => Some(hooks),
            _ => None,
        };
        crate::agent::codex::config_on(&BTreeMap::new(), keys_now, hooks, windows)
    }

    /// Whether the writable `root` is the data folder or Codex's home, holds either, or lies in
    /// Codex's home.
    fn too_wide(&self, root: &Path) -> bool {
        self.data.starts_with(root) || self.home.starts_with(root) || root.starts_with(&self.home)
    }
}

impl Drop for Launch {
    fn drop(&mut self) {
        drop(write::remove_dir_all(&self.root));
    }
}

/// Every place `config` makes writable: `sandbox_workspace_write.writable_roots`, and each
/// `write` entry of the permission profile, a scoped one under its base.
fn writable(config: &Map<String, Value>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = config["sandbox_workspace_write"]["writable_roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|root| PathBuf::from(root.as_str().unwrap()))
        .collect();
    let filesystem = config
        .get("permissions")
        .and_then(|permissions| permissions[super::PROFILE]["filesystem"].as_object());
    for (path, mode) in filesystem.into_iter().flatten() {
        match mode {
            Value::String(mode) if mode == "write" => out.push(PathBuf::from(path)),
            Value::Object(scoped) => {
                for (relative, mode) in scoped {
                    if mode == "write" {
                        out.push(Path::new(path).join(relative));
                    }
                }
            }
            Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Array(_) => {}
        }
    }
    out
}

/// On macOS and Linux: the profile extends `:workspace` and writes only the run folder and the
/// notes; nothing writable is the data folder, Codex's home, above either or in the home, and
/// neither is a `write` entry. The ask mode and ACP's `additionalDirectories` say the same.
#[test]
fn the_codex_launch_never_makes_codex_home_or_the_data_folder_writable_off_windows() {
    let launch = Launch::new("writable-unix");
    let config = launch.config(false);
    assert_eq!(config["default_permissions"], super::PROFILE);
    assert_eq!(
        config["permissions"][super::PROFILE]["extends"],
        super::EXTENDS
    );
    let roots = writable(&config);
    assert_ne!(roots, Vec::<PathBuf>::new());
    for root in &roots {
        assert!(!launch.too_wide(root), "{root:?} in {config:?}");
    }
    let mut writes: Vec<&PathBuf> = roots
        .iter()
        .filter(|root| !root.ends_with("work"))
        .collect();
    writes.sort();
    writes.dedup();
    assert_eq!(writes, vec![&launch.notes, &launch.run_root]);
    assert!(config.get("windows").is_none());
    assert_eq!(crate::agent::codex::ASK_MODE, "workspace-write");
    for directory in [&launch.run_root, &launch.notes] {
        assert!(!launch.too_wide(directory));
    }
}

/// On Windows: no permission profile, the unelevated sandbox, and writable roots that are
/// neither the data folder nor Codex's home, nor hold or lie in either.
#[test]
fn the_codex_launch_never_makes_codex_home_or_the_data_folder_writable_on_windows() {
    let launch = Launch::new("writable-windows");
    let config = launch.config(true);
    assert!(config.get("default_permissions").is_none());
    assert!(config.get("permissions").is_none());
    assert_eq!(
        config["windows"]["sandbox"],
        crate::agent::codex::WINDOWS_SANDBOX
    );
    let roots = writable(&config);
    assert_ne!(roots, Vec::<PathBuf>::new());
    for root in roots.iter().chain([&launch.run_root, &launch.notes]) {
        assert!(!launch.too_wide(root), "{root:?} in {config:?}");
    }
    assert!(config.get("sandbox_mode").is_none());
}

/// What Codex loads settings, rules, instructions or code from in its home is denied to every
/// command, ordinary or approved, read and write alike; its working folders stay readable.
#[test]
fn what_codex_loads_from_its_home_is_denied_to_every_command() {
    let keys = super::tests::fence();
    let home = super::tests::codex_home();
    let loaded = crate::agent::codex::home_clear::LOADED;
    let names = ["config.toml", "rules"].into_iter().chain(loaded);
    for name in names {
        for path in [home.join(name), home.join(name).join("x")] {
            let shown = path.display();
            assert_eq!(super::tests::access(&keys, &path), "deny", "{shown}");
            assert_eq!(
                super::tests::escalated_access(&keys, &path),
                "deny",
                "{shown}"
            );
        }
    }
    let arg0 = home.join("tmp").join("arg0").join("codex-linux-sandbox");
    assert!(super::tests::readable(&keys, &arg0));
}
