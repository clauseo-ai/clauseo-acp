//! The sign-in modes over real processes: `claude-cli` and `codex.js`, each run with a temp `HOME`.
//! A mode is picked by its script's file name, so each run passes an empty file of that name as
//! argv[1], as the engine will. Nothing here sleeps: each wait is a read of a line the mode
//! prints, or of its exit.

use std::path::{Path, PathBuf};

use clauseo_fake_agent::{EXIT_LOAD, EXIT_OK};
use serde_json::Value;

use crate::harness::{Agent, Scratch};
use crate::write;

#[path = "signin/claude_cli.rs"]
mod claude_cli;
#[path = "signin/codex_cli.rs"]
mod codex_cli;

/// The marker's file name, as the modes write it.
const MARKER: &str = "fake-signin-marker";
const CLAUDE_MANUAL_LINE: &str = "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?redirect_uri=https://platform.claude.com/oauth/code/callback";
const CLAUDE_PROMPT: &str = "Paste code here if prompted >";
const CLAUDE_STATUS_OUT: &str =
    r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#;
const CLAUDE_LOGOUT: &str = "Successfully logged out from your Claude account.";
const CODEX_CALLBACK_LINE: &str =
    "Starting local login server on http://127.0.0.1:1455/auth/callback";
const CODEX_DEVICE_LINE: &str =
    "To sign in, open https://auth.openai.com/codex/device and enter the code ABCD-EFGH";
const CODEX_REFUSAL: &str = "Enable device code sign-in for Codex, Excel, PowerPoint, and Word in ChatGPT Security Settings, then run codex login --device-auth again.";

/// A temp `HOME` under `scratch`, as text.
fn home(scratch: &Scratch) -> String {
    let home = scratch.path().join("home");
    write::create_dir_all(&home).unwrap();
    text(&home).to_owned()
}

/// `path` as text.
fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

/// `scratch/relative`, an empty file: the script a run names as argv[1]. Returns its path as text.
fn script(scratch: &Scratch, relative: &str) -> String {
    let path = scratch.path().join(relative);
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write(&path, "").unwrap();
    text(&path).to_owned()
}

/// Starts `<script> <args>` with `HOME` and `USERPROFILE` (the home the fake reads on Windows)
/// set to `home`, and `env` set on top.
fn start(
    scratch: &Scratch,
    relative: &str,
    home: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Agent {
    let script = script(scratch, relative);
    let mut argv = vec![script.as_str()];
    argv.extend_from_slice(args);
    let mut vars = vec![("HOME", home), ("USERPROFILE", home)];
    vars.extend_from_slice(env);
    Agent::start_args(&argv, scratch.path(), &vars)
}

/// The Claude mode, from a script named `claude-cli` in a folder of its own.
fn claude(scratch: &Scratch, home: &str, args: &[&str], env: &[(&str, &str)]) -> Agent {
    start(scratch, "install/bin/claude-cli", home, args, env)
}

/// The Codex mode, from a script named `codex.js` under an npm folder, as the engine runs it.
fn codex(scratch: &Scratch, home: &str, args: &[&str], env: &[(&str, &str)]) -> Agent {
    start(
        scratch,
        "install/node_modules/@openai/codex/bin/codex.js",
        home,
        args,
        env,
    )
}

/// The folder Claude keeps its marker in when no variable moves it.
fn claude_dir(home: &str) -> PathBuf {
    PathBuf::from(home).join(".claude")
}

/// The marker in `folder`, whether or not it is there.
fn marker(folder: &Path) -> PathBuf {
    folder.join(MARKER)
}

/// The marker in `folder`, parsed.
fn read_marker(folder: &Path) -> Value {
    let text = std::fs::read_to_string(marker(folder)).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// Claude's `auth status --json` from a fresh run, which must exit 0.
fn claude_status(scratch: &Scratch, home: &str, env: &[(&str, &str)]) -> String {
    let finished = claude(scratch, home, &["auth", "status", "--json"], env).finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    finished.stdout
}

/// Codex's `login status` from a fresh run, which must exit with `code`. Returns its stdout.
fn codex_status(scratch: &Scratch, home: &str, env: &[(&str, &str)], code: i32) -> String {
    let finished = codex(scratch, home, &["login", "status"], env).finish();
    assert_eq!(finished.status.code(), Some(code), "{}", finished.stderr);
    finished.stdout
}

#[test]
fn env_names_file_lists_names_sorted_and_never_values() {
    let scratch = Scratch::new("env_names");
    let home = home(&scratch);
    let out = scratch.path().join("env-names.txt");
    let env = [
        ("FAKE_PROBE", "probe-value-7f3a"),
        ("FAKE_SIGNIN_ENV_OUT", text(&out)),
    ];
    let finished = claude(&scratch, &home, &["auth", "status", "--json"], &env).finish();
    assert_eq!(finished.status.code(), Some(EXIT_OK), "{}", finished.stderr);
    let codex_finished = codex(&scratch, &home, &["login", "status"], &env).finish();
    assert_eq!(
        codex_finished.status.code(),
        Some(1),
        "{}",
        codex_finished.stderr
    );
    let text_out = std::fs::read_to_string(&out).unwrap();
    let names: Vec<&str> = text_out.lines().collect();
    assert!(names.contains(&"FAKE_PROBE"), "{names:?}");
    assert!(names.contains(&"FAKE_SIGNIN_ENV_OUT"), "{names:?}");
    assert!(names.contains(&"HOME"), "{names:?}");
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);
    assert!(text_out.ends_with('\n'));
    assert!(!text_out.contains("probe-value-7f3a"));
    assert!(!text_out.contains(&home));
}

#[test]
fn unknown_commands_exit_2_with_the_usage_line() {
    let scratch = Scratch::new("signin_usage");
    let home = home(&scratch);
    let claude_finished = claude(&scratch, &home, &["auth", "whoami"], &[]).finish();
    assert_eq!(claude_finished.status.code(), Some(EXIT_LOAD));
    assert!(
        claude_finished
            .stderr
            .starts_with("fake_agent: claude-cli takes")
    );
    assert_eq!(claude_finished.stdout, "");
    let codex_finished = codex(&scratch, &home, &["whoami"], &[]).finish();
    assert_eq!(codex_finished.status.code(), Some(EXIT_LOAD));
    assert!(
        codex_finished
            .stderr
            .starts_with("fake_agent: codex.js takes")
    );
}
