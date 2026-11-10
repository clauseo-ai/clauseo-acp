//! Claude Code's own sandbox in a Claude session's flag settings (`agent/loads/sandbox.rs`).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login, claude_options};
use super::sandbox::{ClaudeSandbox, EXCLUDED_COMMANDS, SandboxPaths};
use crate::agent::data_fence::LaunchDeny;

static NO_DENY: LaunchDeny = LaunchDeny {
    spellings: Vec::new(),
    targets: Vec::new(),
};

/// The paths of session `id` of the matter at `/Users/l/Clients/A`, with `<data>` under the
/// lawyer's home as on a Mac.
struct Session {
    matter: PathBuf,
    run: PathBuf,
    tmp: PathBuf,
    work: PathBuf,
    downloads: PathBuf,
    skill_packs: PathBuf,
    notes: PathBuf,
    skills: Vec<PathBuf>,
    path: Vec<PathBuf>,
    home: PathBuf,
}

const DATA: &str = "/Users/l/Library/Application Support/Clauseo";

impl Session {
    fn new(id: &str) -> Self {
        let data = Path::new(DATA);
        let run = data.join("run").join(id);
        Self {
            matter: PathBuf::from("/Users/l/Clients/A"),
            tmp: run.join("tmp"),
            work: run.join("work"),
            downloads: run.join("downloads"),
            skill_packs: run.join("skill-packs"),
            notes: data.join("agent-state/m-1/notes"),
            skills: vec![data.join("profile/claude-plugin/skills")],
            path: vec![
                PathBuf::from("/Applications/Clauseo.app/Contents/Helpers/stetdoc"),
                data.join("agents/runtime/clauseo-python-3.13.9-aarch64/bin"),
            ],
            home: data.join("agents/claude-acp/home"),
            run,
        }
    }

    fn paths(&self, browser: bool) -> SandboxPaths<'_> {
        SandboxPaths {
            matter: &self.matter,
            work: &self.work,
            tmp: &self.tmp,
            downloads: &self.downloads,
            skill_packs: &self.skill_packs,
            notes: &self.notes,
            skills: &self.skills,
            path: &self.path,
            claude_home: &self.home,
            browser,
        }
    }
}

fn folders(session: &Session) -> Folders<'_> {
    Folders {
        work: session.work.to_str().unwrap(),
        notes: session.notes.to_str().unwrap(),
        connectors: "",
        browser: "",
    }
}

/// The flag settings a Claude session gets with `sandbox`, in forced ask or not, over `hook`.
fn settings(
    session: &Session,
    sandbox: Option<&ClaudeSandbox>,
    forced_ask: bool,
    browser: bool,
    hook: &Value,
) -> Value {
    let keys = ClaudeKeys {
        web_search: Switch {
            on: true,
            locked: false,
        },
        login: Login::FirstParty,
        folders: folders(session),
        seed_sites: &["law.example"],
        forced_ask,
        data: &NO_DENY,
        case_law: false,
        browser,
        sandbox,
    };
    let mut options = hook.as_object().cloned().unwrap_or_default();
    claude_options(&mut options, &keys);
    options["settings"].clone()
}

fn texts(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

/// macOS (and Linux, for development): the whole object, each path the session's own.
#[test]
fn a_claude_session_runs_its_commands_in_claude_codes_sandbox() {
    let session = Session::new("s-1");
    let sandbox = ClaudeSandbox::for_os(false, &session.paths(true)).unwrap();
    let settings = settings(&session, Some(&sandbox), false, true, &json!({}));
    let run = session.run.to_str().unwrap();
    assert_eq!(
        settings["sandbox"],
        json!({
            "enabled": true,
            "autoAllowBashIfSandboxed": true,
            "allowUnsandboxedCommands": false,
            "failIfUnavailable": false,
            "allowAppleEvents": false,
            "excludedCommands": ["agent-browser:*"],
            "network": {"allowedDomains": []},
            "filesystem": {
                "allowWrite": [
                    "/Users/l/Clients/A",
                    format!("{run}/work"),
                    format!("{run}/tmp"),
                    format!("{DATA}/agent-state/m-1/notes"),
                ],
                "denyRead": ["~/"],
                "allowRead": [
                    "/Users/l/Clients/A",
                    format!("{run}/work"),
                    format!("{run}/tmp"),
                    format!("{run}/downloads"),
                    format!("{run}/skill-packs"),
                    format!("{DATA}/agent-state/m-1/notes"),
                    format!("{DATA}/profile/claude-plugin/skills"),
                    "/Applications/Clauseo.app/Contents/Helpers/stetdoc",
                    format!("{DATA}/agents/runtime/clauseo-python-3.13.9-aarch64"),
                ],
            },
            "ignoreViolations": {"*": [format!("{DATA}/agents/claude-acp/home/shell-snapshots")]},
        })
    );
}

/// Each session's sandbox names its own run folder, and never another's.
#[test]
fn two_sessions_get_two_sandboxes() {
    let (one, two) = (Session::new("s-1"), Session::new("s-2"));
    let first = ClaudeSandbox::of(&one.paths(false));
    let second = ClaudeSandbox::of(&two.paths(false));
    let run_two = two.run.to_str().unwrap();
    for path in first.allow_read.iter().chain(&first.allow_write) {
        assert!(!path.starts_with(run_two), "{path}");
    }
    assert_ne!(first.allow_write, second.allow_write);
}

/// The run folder holds the relay's token and the browser's profile: only its named folders
/// open, and a `PATH` folder's parent only when it is the Python runtime's.
#[test]
fn the_run_folder_and_the_home_never_open_whole() {
    let mut session = Session::new("s-1");
    session.path.push(PathBuf::from("/Users/l/bin"));
    let sandbox = ClaudeSandbox::of(&session.paths(true));
    let run = session.run.to_str().unwrap();
    assert!(!sandbox.allow_read.iter().any(|path| path == run));
    for path in &sandbox.allow_read {
        assert!(!path.ends_with("relay-token") && !path.ends_with("browser-profile"));
        assert!(path != "/Users/l" && path != "/Users/l/", "{path}");
    }
    assert!(sandbox.allow_read.iter().any(|path| path == "/Users/l/bin"));
}

/// Windows: no `sandbox` key at all, so Claude's commands keep asking there.
#[test]
fn windows_gets_no_sandbox() {
    let session = Session::new("s-1");
    assert_eq!(ClaudeSandbox::for_os(true, &session.paths(true)), None);
    let settings = settings(&session, None, false, true, &json!({}));
    assert!(settings.get("sandbox").is_none(), "{settings}");
}

/// Forced ask comes after the sandbox: it stays on, but nothing runs without asking.
#[test]
fn forced_ask_still_asks_inside_the_sandbox() {
    let session = Session::new("s-1");
    let sandbox = ClaudeSandbox::of(&session.paths(true));
    let settings = settings(&session, Some(&sandbox), true, true, &json!({}));
    let object = &settings["sandbox"];
    assert_eq!(object["autoAllowBashIfSandboxed"], false);
    assert_eq!(object["enabled"], true);
    assert_eq!(object["allowUnsandboxedCommands"], false);
    assert_eq!(object["filesystem"]["denyRead"], json!(["~/"]));
}

/// No command breaks out: the model's `dangerouslyDisableSandbox` is ignored, whatever the hook
/// sent, and the hook's own `sandbox` goes whole.
#[test]
fn no_command_breaks_out_and_the_hooks_sandbox_is_replaced() {
    let session = Session::new("s-1");
    let sandbox = ClaudeSandbox::of(&session.paths(false));
    let hook = json!({"settings": {"sandbox": {
        "allowUnsandboxedCommands": true,
        "excludedCommands": ["*"],
        "allowAppleEvents": true,
        "filesystem": {"allowWrite": ["/"]},
    }}});
    let settings = settings(&session, Some(&sandbox), false, false, &hook);
    let object = &settings["sandbox"];
    assert_eq!(object["allowUnsandboxedCommands"], false);
    assert_eq!(object["allowAppleEvents"], false);
    assert!(object.get("excludedCommands").is_none(), "{object}");
    assert!(!texts(&object["filesystem"]["allowWrite"]).contains(&"/"));
}

/// The one exclusion is agent-browser's prefix rule, which Claude Code 2.1.293 honours only
/// when every part of a command is agent-browser (`agent-browser snapshot && touch ~/x` runs
/// sandboxed); and only in a session with the browser.
#[test]
fn only_agent_browser_runs_outside_the_sandbox_and_only_with_the_browser() {
    assert_eq!(EXCLUDED_COMMANDS, ["agent-browser:*"]);
    for rule in EXCLUDED_COMMANDS {
        let prefix = rule.strip_suffix(":*").unwrap();
        assert!(
            !prefix.contains(['*', ' ', ';', '&', '|', '$', '`']),
            "{rule}"
        );
    }
    let session = Session::new("s-1");
    let with = ClaudeSandbox::of(&session.paths(true)).settings();
    assert_eq!(with["excludedCommands"], json!(EXCLUDED_COMMANDS));
    let without = ClaudeSandbox::of(&session.paths(false)).settings();
    assert!(without.get("excludedCommands").is_none());
}

/// A path that is not UTF-8 is left out: neither readable nor writable.
#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_is_left_out() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let mut session = Session::new("s-1");
    session.matter = PathBuf::from(OsStr::from_bytes(b"/Users/l/\xff"));
    let sandbox = ClaudeSandbox::of(&session.paths(false));
    assert_eq!(sandbox.allow_write.len(), 3);
    assert!(
        !sandbox
            .allow_read
            .iter()
            .any(|path| path.contains("Users/l/\u{fffd}"))
    );
}
