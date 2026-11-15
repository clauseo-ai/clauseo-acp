//! The lawyer's local MCP servers start through `clauseo-mcp launch <file>`
//! (`actor/turn/profile_loads/mcp_launch.rs`): with the profile's `clauseo-mcp`, `session/new`
//! carries the launcher, `launch` and the file's path, and no argument or variable of the
//! server; the file, private to the user, holds them, keychain value included, and stays for
//! the run. Without a `clauseo-mcp` the server goes as before (`mcp_tools.rs`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_host::ProfileHook;
use clauseo_protocol::AgentId;
use serde_json::{Map, Value, json};

use crate::mcp_tools::{HELD, account, finds_npx, npx, store, wired};
use crate::profile_rig::{agent_lines, template, turn};
use crate::support::{Dir, FAKE};

/// A hook that adds nothing but the launcher.
struct Launcher(PathBuf);

impl ProfileHook for Launcher {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn mcp_launcher(&self) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

/// The launch files under the data folder's run folders.
fn launch_files(data: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for run in std::fs::read_dir(data.join("run")).unwrap() {
        let folder = run.unwrap().path().join("mcp");
        if let Ok(entries) = std::fs::read_dir(&folder) {
            files.extend(entries.map(|entry| entry.unwrap().path()));
        }
    }
    files
}

/// `session/new` gets `clauseo-mcp launch <run>/mcp/0-github.json` with no variable (but, on
/// Windows, the host's basics), and that file, 0600 on Unix, holds the resolved program, the
/// arguments and every variable, the keychain value included.
#[test]
fn a_local_server_goes_as_the_launcher_and_its_values_go_to_a_private_file() {
    let dir = Dir::new("mcp_launch_entry", &[]);
    let npx = npx(&dir);
    let launcher = dir.folder("app", &[]).join("clauseo-mcp");
    let env = if cfg!(windows) {
        json!("{{any}}")
    } else {
        json!([])
    };
    let servers = json!([{"name": "github", "command": launcher.display().to_string(),
        "args": ["launch", "{{any}}"], "env": env}]);
    let params = json!({"cwd": "{{any}}", "mcpServers": servers});
    let name = "mcp-launch-entry";
    let scenario = template(&dir, name, &agent_lines(name, "session/new", &params, &[]));
    dir.set_settings(
        &[(FAKE, scenario)],
        json!({"mcp": {"servers": {"github": {"command": "npx", "args": ["-y", "server-github"],
            "env": {"TOKEN": {"keychain": true}, "REGION": "uk"}, "enabled": true}}}}),
    );
    let store = store(&[(&account("github", "TOKEN"), HELD)]);
    let host = dir.start_with(0, |config| {
        wired(config, &store, finds_npx(&npx)).with_profile_hook(Box::new(Launcher(launcher)))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[("brief.txt", "Clause 4\n")]));
    turn(&host, host.create_thread(matter, FAKE));

    // The session is still up: its run folder, and the file in it, stay for it.
    let files = launch_files(dir.data());
    assert_eq!(files.len(), 1, "{files:?}");
    let file = &files[0];
    assert_eq!(file.file_name().unwrap(), "0-github.json");
    let body: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(body["v"], json!(1));
    assert_eq!(body["command"], json!(npx.display().to_string()));
    assert_eq!(body["args"], json!(["-y", "server-github"]));
    let env = body["env"].as_array().unwrap();
    assert!(env.contains(&json!(["TOKEN", HELD])), "{env:?}");
    assert!(env.contains(&json!(["REGION", "uk"])), "{env:?}");
    assert!(
        env.iter()
            .any(|pair| pair[0].as_str().unwrap().eq_ignore_ascii_case("PATH"))
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{mode:o}");
        let folder = std::fs::metadata(file.parent().unwrap()).unwrap();
        assert_eq!(folder.permissions().mode() & 0o777, 0o700);
    }
    host.shutdown();
}
