//! Claude Code's own sandbox in a Claude session's launch (`agent::loads::sandbox`).

use std::sync::Arc;

use serde_json::{Value, json};

use crate::profile_rig::{CLAUDE, Wired, agent_lines, agents, settings, template, turn};
use crate::support::Dir;

/// Claude Code's own sandbox (`agent::loads::sandbox`): a Claude session on macOS and Linux
/// gets it, with its work folder writable, the home denied and a short temp folder of its own
/// as `CLAUDE_CODE_TMPDIR`; on Windows none, so its commands keep asking.
#[test]
fn claude_runs_its_commands_in_its_sandbox_except_on_windows() {
    let dir = Dir::new("profile_claude_sandbox", &[]);
    let (expected, offs): (Value, &[&str]) = if cfg!(windows) {
        (json!({}), &["/_meta/claudeCode/options/settings/sandbox"])
    } else {
        (
            json!({"sandbox": {
                "enabled": true,
                "autoAllowBashIfSandboxed": true,
                "allowUnsandboxedCommands": false,
                "filesystem": {
                    "denyRead": ["~/", "/Volumes", "/Users/Shared", "{{any}}", "{{any}}"],
                    "denyWrite": ["{{any}}"],
                    "allowWrite": ["{{any}}", "@WORK@", "{{any}}", "{{any}}", "{{any}}"],
                },
            }}),
            &[],
        )
    };
    let claude = template(
        &dir,
        "sandbox",
        &agent_lines(
            "sandbox",
            "session/new",
            &json!({"_meta": {"claudeCode": {"options": {"settings": expected}}}}),
            offs,
        ),
    );
    settings(&dir, agents(&[(CLAUDE, &claude, json!({}))]), json!({}));
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| config);
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    host.shutdown();
    let seen = launcher.seen();
    let launch = seen.first().expect("a Claude launch");
    let tmp = launch
        .env
        .get(&std::ffi::OsString::from("CLAUDE_CODE_TMPDIR"));
    if cfg!(windows) {
        assert_eq!(tmp, None);
    } else {
        // Claude's short temp folder, gone with the run folder once the session ended.
        let short = std::path::PathBuf::from(tmp.expect("CLAUDE_CODE_TMPDIR"));
        assert!(short.starts_with("/tmp"), "{}", short.display());
        assert!(short.join("claude-4294967295").as_os_str().len() <= 44);
        assert!(!short.exists(), "removed with the run folder");
    }
}
