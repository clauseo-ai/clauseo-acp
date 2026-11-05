use std::collections::BTreeMap;
use std::ffi::OsString;

use clauseo_settings::AgentLaunch;

use crate::agent::launch_spec;

use super::absolute_program;

/// The spec is the child's whole environment (the launcher clears the host's), so it drops every
/// inherited `GIT_*`, `SSH_*`, `NO_BROWSER` and `CLAUDE_CODE_REMOTE` in any case, and keeps what
/// an agent needs to find its tools and config: `PATH`, `HOME`, and Windows's `USERPROFILE`,
/// `APPDATA`, `LOCALAPPDATA`, `SystemRoot` and `PATHEXT` (plain names; the spec logic is the same
/// on every OS).
#[test]
fn launch_spec_drops_remote_markers_and_keeps_what_agents_need() {
    let agent = AgentLaunch {
        command: absolute_program(),
        args: Vec::new(),
        env: BTreeMap::new(),
    };
    let kept = [
        ("APPDATA", "C:\\Users\\ada\\AppData\\Roaming"),
        ("HOME", "/home/ada"),
        ("LOCALAPPDATA", "C:\\Users\\ada\\AppData\\Local"),
        ("PATH", "/usr/bin"),
        ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
        ("SSHFS_OPTS", "kept: not an SSH_ prefix"),
        ("SystemRoot", "C:\\Windows"),
        ("USERPROFILE", "C:\\Users\\ada"),
    ];
    let dropped = [
        ("CLAUDE_CODE_REMOTE", "true"),
        ("GIT_DIR", "/elsewhere/.git"),
        ("GIT_WORK_TREE", "/elsewhere"),
        ("No_Browser", "1"),
        ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
        ("SSH_CONNECTION", "10.0.0.1 22 10.0.0.2 22"),
        ("ssh_tty", "/dev/pts/0"),
    ];
    let inherited = kept
        .iter()
        .chain(&dropped)
        .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    let spec = launch_spec(
        &agent,
        BTreeMap::new(),
        BTreeMap::new(),
        &[],
        inherited,
        std::env::temp_dir(),
        None,
    )
    .unwrap();
    let env: Vec<_> = spec
        .env()
        .iter()
        .map(|(k, v)| (k.to_str().unwrap(), v.to_str().unwrap()))
        .collect();
    assert_eq!(env, kept);
}

/// The drop lists, as `crates/limits/src/readers/env.rs` copies them for the plan readers: a change
/// to one goes in both.
#[test]
fn the_drop_lists_are_the_plan_readers() {
    assert_eq!(
        crate::agent::spec::DROPPED_INHERITED_PREFIXES,
        ["GIT_", "SSH_"]
    );
    let markers = crate::agent::spec::DROPPED_INHERITED.join(" ");
    assert_eq!(markers, "NO_BROWSER CLAUDE_CODE_REMOTE");
    let codex = crate::agent::codex::DROPPED_VARS.join(" ");
    let expected = "CODEX_PATH APP_SERVER_LOGS DEFAULT_AUTH_REQUEST MODEL_PROVIDER";
    assert_eq!(codex, expected);
}
