//! Claude Code's own sandbox in a Claude session's flag settings (`agent/loads/sandbox.rs`).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login, claude_options};
use super::sandbox::{ClaudeSandbox, Off, SandboxOs, SandboxPaths};
use crate::agent::data_fence::LaunchDeny;

static NO_DENY: LaunchDeny = LaunchDeny {
    spellings: Vec::new(),
    targets: Vec::new(),
    kept: Vec::new(),
    own_reads: Vec::new(),
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
    env_file: PathBuf,
    ca_file: Option<PathBuf>,
    home: PathBuf,
    claude_tmp: Option<PathBuf>,
    data: Vec<PathBuf>,
    other_matters: Vec<PathBuf>,
}

const DATA: &str = "/Users/l/Library/Application Support/Clauseo";
const SHORT: &str = "/tmp/clsb-abcd1234";
const PROJECT: &str = "/Users/l/Library/Application Support/Clauseo/agents/claude-acp/home/projects/-Users-l-Clients-A";

impl Session {
    fn new(id: &str) -> Self {
        let data = Path::new(DATA);
        let run = data.join("run").join(id);
        Self {
            matter: PathBuf::from("/Users/l/Clients/A"),
            tmp: run.join("tmp"),
            work: run.join("work"),
            downloads: run.join("agent-browser/downloads"),
            skill_packs: run.join("skill-packs"),
            notes: data.join("agent-state/m-1/notes"),
            skills: vec![data.join("profile/claude-plugin/skills")],
            path: vec![
                PathBuf::from("/Applications/Clauseo.app/Contents/Helpers/stetdoc"),
                data.join("agents/runtime/clauseo-python-3.13.9-aarch64/bin"),
            ],
            env_file: run.join("clauseo-env.sh"),
            ca_file: Some(data.join("tls/roots.pem")),
            home: data.join("agents/claude-acp/home"),
            claude_tmp: Some(PathBuf::from(SHORT)),
            data: vec![data.to_path_buf()],
            other_matters: vec![PathBuf::from("/Users/l/Clients/B")],
            run,
        }
    }

    fn paths(&self) -> SandboxPaths<'_> {
        SandboxPaths {
            matter: &self.matter,
            work: &self.work,
            tmp: &self.tmp,
            claude_tmp: self.claude_tmp.as_deref(),
            downloads: &self.downloads,
            skill_packs: &self.skill_packs,
            notes: &self.notes,
            skills: &self.skills,
            path: &self.path,
            env_file: Some(&self.env_file),
            ca_file: self.ca_file.as_deref(),
            claude_home: &self.home,
            data: &self.data,
            other_matters: &self.other_matters,
        }
    }
}

/// The sandbox for `paths` on macOS.
fn mac(paths: &SandboxPaths<'_>) -> Result<ClaudeSandbox, Off> {
    ClaudeSandbox::for_os(SandboxOs::Mac, paths)
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
        matter: None,
        trusted_servers: &[],
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
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn a_claude_session_runs_its_commands_in_claude_codes_sandbox() {
    let session = Session::new("s-1");
    let sandbox = ClaudeSandbox::for_os(SandboxOs::Mac, &session.paths()).unwrap();
    let settings = settings(&session, Some(&sandbox), false, false, &json!({}));
    let run = session.run.to_str().unwrap();
    assert_eq!(
        settings["sandbox"],
        json!({
            "enabled": true,
            "autoAllowBashIfSandboxed": true,
            "allowUnsandboxedCommands": false,
            "failIfUnavailable": true,
            "allowAppleEvents": false,
            "network": {
                "allowedDomains": ["*"],
                "deniedDomains": [
                    "localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0", "[::]",
                ],
            },
            "filesystem": {
                "allowWrite": [
                    "/Users/l/Clients/A",
                    format!("{run}/work"),
                    format!("{run}/tmp"),
                    format!("{DATA}/agent-state/m-1/notes"),
                    SHORT,
                ],
                "denyWrite": ["/Users/l/Clients/A/.clauseo"],
                "denyRead": ["~/", "/Volumes", "/Users/Shared", DATA, "/Users/l/Clients/B"],
                "allowRead": [
                    "/Users/l/Clients/A",
                    format!("{run}/work"),
                    format!("{run}/tmp"),
                    SHORT,
                    format!("{run}/agent-browser/downloads"),
                    format!("{run}/skill-packs"),
                    format!("{DATA}/agent-state/m-1/notes"),
                    format!("{DATA}/profile/claude-plugin/skills"),
                    format!("{run}/clauseo-env.sh"),
                    format!("{DATA}/tls/roots.pem"),
                    "/Applications/Clauseo.app/Contents/Helpers/stetdoc",
                    format!("{DATA}/agents/runtime/clauseo-python-3.13.9-aarch64"),
                    format!("{PROJECT}/*/tool-results"),
                    format!("{PROJECT}/*/tool-results/**"),
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
    let first = mac(&one.paths()).unwrap();
    let second = mac(&two.paths()).unwrap();
    let run_two = two.run.to_str().unwrap();
    for path in first.allow_read.iter().chain(&first.allow_write) {
        assert!(!path.starts_with(run_two), "{path}");
    }
    assert_ne!(first.allow_write, second.allow_write);
}

/// A `PATH` folder's parent opens only when it is the Python runtime's, never the home. (The
/// real run folder's layout, relay token and browser profile included, is checked against
/// `RunFolders` in `launch_checks/tests.rs`.)
#[test]
fn the_home_never_opens_through_a_path_folder() {
    let mut session = Session::new("s-1");
    session.path.push(PathBuf::from("/Users/l/bin"));
    let sandbox = mac(&session.paths()).unwrap();
    for path in &sandbox.allow_read {
        assert!(path != "/Users/l" && path != "/Users/l/", "{path}");
    }
    assert!(sandbox.allow_read.iter().any(|path| path == "/Users/l/bin"));
}

/// A data folder outside the home (`CLAUSEO_DATA_DIR`), in each spelling, and every other
/// matter's folder are denied; the current matter on a shared volume stays open (`allowRead`
/// wins over `/Volumes`).
#[test]
fn the_data_folder_and_other_matters_are_denied_wherever_they_are() {
    let mut session = Session::new("s-1");
    session.data = vec![PathBuf::from("/srv/clauseo"), PathBuf::from("/srv/link")];
    session.matter = PathBuf::from("/Volumes/x/Clients/A");
    session.other_matters = vec![
        PathBuf::from("/Volumes/x/Clients/B"),
        PathBuf::from("/opt/C"),
    ];
    let sandbox = mac(&session.paths()).unwrap();
    for denied in [
        "/srv/clauseo",
        "/srv/link",
        "/Volumes/x/Clients/B",
        "/opt/C",
        "/Volumes",
        "/Users/Shared",
        "~/",
    ] {
        assert!(
            sandbox.deny_read.iter().any(|path| path == denied),
            "{denied}"
        );
    }
    assert!(
        !sandbox
            .deny_read
            .iter()
            .any(|path| path == "/Volumes/x/Clients/A")
    );
    assert!(
        sandbox
            .allow_read
            .iter()
            .any(|path| path == "/Volumes/x/Clients/A")
    );
    assert!(
        sandbox
            .allow_write
            .iter()
            .any(|path| path == "/Volumes/x/Clients/A")
    );
}

/// `*` or `?` in any path the lists would hold turns the sandbox off: Claude Code reads both as
/// a pattern with no escape. The caller then holds the session off `auto` (`Off::Wildcard`).
#[test]
fn a_wildcard_in_any_path_sends_no_sandbox() {
    for glob in ["A*", "Q?", "[A*]"] {
        let mut session = Session::new("s-1");
        session.matter = PathBuf::from("/Users/l/Clients").join(glob);
        for os in [SandboxOs::Mac, SandboxOs::Linux] {
            let off = ClaudeSandbox::for_os(os, &session.paths());
            assert_eq!(off, Err(Off::Wildcard), "{glob}");
        }
        let mut session = Session::new("s-1");
        session
            .other_matters
            .push(PathBuf::from("/Users/l/Clients").join(glob));
        assert_eq!(mac(&session.paths()), Err(Off::Wildcard), "{glob}");
        let settings = settings(&session, None, false, false, &json!({}));
        assert!(settings.get("sandbox").is_none());
    }
    assert!(mac(&Session::new("s-1").paths()).is_ok());
}

/// A `[` keeps the sandbox on, escaped as `[[]` where Claude Code reads globs: every list on
/// macOS, only the read lists on Linux, whose write lists take no glob. Each escaped allowed
/// folder also goes in with `/**`, since a glob allow names only itself.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn a_bracket_is_escaped_and_the_sandbox_stays_on() {
    let mut session = Session::new("s-1");
    session.matter = PathBuf::from("/Users/l/Clients/Smith [2024]");
    session.other_matters = vec![PathBuf::from("/Users/l/Clients/Jones [a-b]")];
    let escaped = "/Users/l/Clients/Smith [[]2024]";
    let on_mac = mac(&session.paths()).unwrap();
    for list in [&on_mac.allow_write, &on_mac.allow_read] {
        assert!(list.iter().any(|p| p == escaped), "{list:?}");
        assert!(
            list.iter().any(|p| *p == format!("{escaped}/**")),
            "{list:?}"
        );
        assert!(
            !list
                .iter()
                .any(|p| p.contains("[2024]") && !p.contains("[[]"))
        );
    }
    assert_eq!(on_mac.deny_write, [format!("{escaped}/.clauseo")]);
    assert!(
        on_mac
            .deny_read
            .iter()
            .any(|p| p == "/Users/l/Clients/Jones [[]a-b]")
    );
    // The project key is Claude Code's own: brackets and spaces as `-`.
    let key = "-Users-l-Clients-Smith--2024-";
    assert!(
        on_mac.allow_read.iter().any(|p| p.contains(key)),
        "{:?}",
        on_mac.allow_read
    );
    let linux = ClaudeSandbox::for_os(SandboxOs::Linux, &session.paths()).unwrap();
    assert!(
        linux
            .allow_write
            .iter()
            .any(|p| p == "/Users/l/Clients/Smith [2024]")
    );
    assert_eq!(linux.deny_write, ["/Users/l/Clients/Smith [2024]/.clauseo"]);
    assert!(linux.allow_read.iter().any(|p| p == escaped));
    assert!(
        linux
            .deny_read
            .iter()
            .any(|p| p == "/Users/l/Clients/Jones [[]a-b]")
    );
    // Without a bracket nothing changes and nothing doubles.
    let plain = mac(&Session::new("s-1").paths()).unwrap();
    assert!(!plain.allow_write.iter().any(|p| p.ends_with("/**")));
}

/// Of Claude's project folder only the tool results of its sessions open, never the transcripts
/// or the memory beside them; the key is `data_fence::claude_project_key`'s, cut and hashed past
/// 200 characters as Claude Code does.
#[test]
fn only_the_projects_tool_results_open() {
    let mut session = Session::new("s-1");
    session.matter = PathBuf::from(format!("/Users/l/{}", "x".repeat(220)));
    let sandbox = mac(&session.paths()).unwrap();
    let home = session.home.to_str().unwrap();
    let key = crate::agent::data_fence::claude_project_key(session.matter.to_str().unwrap());
    assert!(key.len() < 220, "{key}");
    let projects: Vec<&String> = sandbox
        .allow_read
        .iter()
        .filter(|p| p.contains("/projects/"))
        .collect();
    assert_eq!(
        projects,
        [
            &format!("{home}/projects/{key}/*/tool-results"),
            &format!("{home}/projects/{key}/*/tool-results/**"),
        ]
    );
}

/// No short temp folder (it could not be made): no sandbox.
#[test]
fn no_short_temp_folder_sends_no_sandbox() {
    let mut session = Session::new("s-1");
    session.claude_tmp = None;
    assert_eq!(mac(&session.paths()), Err(Off::NoTemp));
}

/// Every reason for no sandbox but Windows holds the session off `auto` (`acceptEdits`), so no
/// command runs on the classifier alone where the sandbox was meant to be.
#[test]
fn no_sandbox_where_one_was_meant_holds_auto_back() {
    for off in [Off::NoTemp, Off::NotUtf8, Off::Wildcard] {
        assert!(off.holds_auto_back(), "{off:?}");
    }
    assert!(!Off::Windows.holds_auto_back());
}

/// Windows: no `sandbox` key at all, so Claude's commands keep asking there.
#[test]
fn windows_gets_no_sandbox() {
    let session = Session::new("s-1");
    let off = ClaudeSandbox::for_os(SandboxOs::Windows, &session.paths());
    assert_eq!(off, Err(Off::Windows));
    let settings = settings(&session, None, false, true, &json!({}));
    assert!(settings.get("sandbox").is_none(), "{settings}");
}

/// Forced ask comes after the sandbox: it stays on, but nothing runs without asking.
#[test]
fn forced_ask_still_asks_inside_the_sandbox() {
    let session = Session::new("s-1");
    let sandbox = mac(&session.paths()).unwrap();
    let settings = settings(&session, Some(&sandbox), true, true, &json!({}));
    let object = &settings["sandbox"];
    assert_eq!(object["autoAllowBashIfSandboxed"], false);
    assert_eq!(object["enabled"], true);
    assert_eq!(object["allowUnsandboxedCommands"], false);
    assert_eq!(object["filesystem"]["denyRead"][0], "~/");
}

/// No command breaks out: the model's `dangerouslyDisableSandbox` is ignored, a sandbox that
/// fails fails the command (`failIfUnavailable`), whatever the hook sent, and the hook's own
/// `sandbox` goes whole.
#[test]
fn no_command_breaks_out_and_the_hooks_sandbox_is_replaced() {
    let session = Session::new("s-1");
    let sandbox = mac(&session.paths()).unwrap();
    let hook = json!({"settings": {"sandbox": {
        "allowUnsandboxedCommands": true,
        "failIfUnavailable": false,
        "excludedCommands": ["*"],
        "allowAppleEvents": true,
        "filesystem": {"allowWrite": ["/"]},
    }}});
    let settings = settings(&session, Some(&sandbox), false, false, &hook);
    let object = &settings["sandbox"];
    assert_eq!(object["allowUnsandboxedCommands"], false);
    assert_eq!(object["failIfUnavailable"], true);
    assert_eq!(object["allowAppleEvents"], false);
    assert!(object.get("excludedCommands").is_none(), "{object}");
    assert!(!texts(&object["filesystem"]["allowWrite"]).contains(&"/"));
}

/// No command runs outside the sandbox: there is no `excludedCommands`, whatever the session's
/// keys, since Claude Code's exclusion check can be steered by a `VAR=` prefix, `env` or quoting
/// (Claude has no browser on macOS and Linux for launch, decision record 0030).
#[test]
fn no_command_is_excluded_from_the_sandbox() {
    let session = Session::new("s-1");
    let sandbox = mac(&session.paths()).unwrap();
    assert!(sandbox.settings().get("excludedCommands").is_none());
    for (forced_ask, browser) in [(false, false), (false, true), (true, true)] {
        let settings = settings(&session, Some(&sandbox), forced_ask, browser, &json!({}));
        assert!(
            settings["sandbox"].get("excludedCommands").is_none(),
            "{settings}"
        );
    }
}

/// An allowed path that is not UTF-8 is left out (neither readable nor writable); a path to
/// deny that is not turns the sandbox off.
#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_fails_closed() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let odd = PathBuf::from(OsStr::from_bytes(b"/Users/l/\xff"));
    let mut session = Session::new("s-1");
    session.matter = odd.clone();
    let sandbox = mac(&session.paths()).unwrap();
    assert_eq!(sandbox.allow_write.len(), 4);
    assert!(
        !sandbox
            .allow_read
            .iter()
            .any(|path| path.contains("Users/l/\u{fffd}"))
    );
    let mut session = Session::new("s-1");
    session.other_matters.push(odd);
    assert_eq!(mac(&session.paths()), Err(Off::NotUtf8));
}

/// Claude may not edit the matter's `.clauseo` folder (its `matter.json`), sandbox or not.
#[cfg(not(windows))]
#[test]
fn the_matter_marker_is_denied_to_claudes_edit() {
    let session = Session::new("s-1");
    let keys = ClaudeKeys {
        web_search: Switch {
            on: false,
            locked: false,
        },
        login: Login::FirstParty,
        folders: folders(&session),
        seed_sites: &[],
        forced_ask: false,
        data: &NO_DENY,
        case_law: false,
        browser: false,
        sandbox: None,
        matter: Some(&session.matter),
        trusted_servers: &[],
    };
    let mut options = serde_json::Map::new();
    claude_options(&mut options, &keys);
    let deny = texts(&options["settings"]["permissions"]["deny"]);
    assert!(
        deny.contains(&"Edit(//Users/l/Clients/A/.clauseo)"),
        "{deny:?}"
    );
    assert!(
        deny.contains(&"Edit(//Users/l/Clients/A/.clauseo/**)"),
        "{deny:?}"
    );
}

/// The trust roots file opens alone, never its folder or `<data>`; without one nothing opens.
#[test]
#[cfg_attr(windows, ignore = "Claude's sandbox is macOS and Linux only")]
fn only_the_trust_roots_file_opens() {
    let mut session = Session::new("s-1");
    let sandbox = mac(&session.paths()).unwrap();
    let file = format!("{DATA}/tls/roots.pem");
    assert!(sandbox.allow_read.contains(&file));
    for folder in [format!("{DATA}/tls"), DATA.to_owned()] {
        assert!(!sandbox.allow_read.contains(&folder), "{folder}");
    }
    session.ca_file = None;
    let sandbox = mac(&session.paths()).unwrap();
    assert!(!sandbox.allow_read.iter().any(|path| path.contains("/tls")));
}
