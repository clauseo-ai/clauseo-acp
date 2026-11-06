//! The sign-in modes: `fake_agent` run as a fake Claude Code CLI or a fake Codex CLI, for the
//! engine tests of S18 (sign-in and status) in `crates/agents`.
//!
//! # Owns
//!
//! - The mode. It is picked by the file name of `argv[1]`, the script the engine runs:
//!   `claude-cli` for Claude Code, `codex.js` for Codex. The engine's argv is then
//!   `<node> <script> <args>`, as in production. The script is never read, so a test may make it
//!   an empty file.
//! - The commands each mode takes, the lines it prints, its exit codes and its marker file.
//! - `FAKE_SIGNIN`, which scripts a sign-in (see [`Script`]), and `FAKE_SIGNIN_ENV_OUT`.
//!
//! # Invariants
//!
//! - The marker is `fake-signin-marker` in the CLI's home folder. It holds
//!   `{"authMethod": ..., "email": "test@example.com"}` and nothing else, never a credential.
//!   Claude's status and Codex's `account/read` print the marker's email. For Codex a test's
//!   dummy file with the name of Codex's login file counts as the marker too (reuse's hard
//!   link, S18).
//! - The Claude home is `CLAUDE_SECURESTORAGE_CONFIG_DIR` when set (`""` is `$HOME/.claude`), else
//!   `CLAUDE_CONFIG_DIR` when set and not empty, else `$HOME/.claude`. Codex's home is `CODEX_HOME`
//!   when set and not empty, else `$HOME/.codex`.
//! - Stdout carries only the lines this module prints, each flushed as it is written.
//!   Diagnostics go to stderr.
//! - The one wait is `FAKE_SIGNIN=ok-after:<ms>`, which sleeps before the marker is written.
//! - File changes go through [`crate::write`], as the rest of the crate's do.
//!
//! # Depends on
//!
//! `serde_json` for the JSON, and `crate::write`. No Clauseo crate.
//!
//! # Start reading
//!
//! [`run`], then [`Script`].

use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};

use crate::write;
use crate::{EXIT_LOAD, EXIT_OK, diagnostic, park_forever};

/// The file name of the marker in the CLI's home folder.
pub(crate) const MARKER: &str = "fake-signin-marker";
/// The name of Codex's own login file, which the fake also takes as its marker (see [`marker`]).
const CODEX_LOGIN_FILE: &str = "auth.json";
/// The marker `authMethod` for which Codex's `account/read` gives an API-key account, with no
/// email (a test's dummy marker; no key is ever written).
const API_KEY_METHOD: &str = "apikey";
/// The email every marker and every status reply carries.
const EMAIL: &str = "test@example.com";
/// The one code `FAKE_SIGNIN=paste` accepts.
const PASTE_CODE: &str = "FAKE-CODE";
/// The exit code of a sign-in that did not finish: cancelled, a refused code, a refused device
/// code, or a busy port. Also the exit of a stdout that closed.
const FAILED: i32 = 1;

/// The URL `$BROWSER` is called with, as Claude's login does.
const CLAUDE_BROWSER_URL: &str = "https://claude.com/cai/oauth/authorize?fake=1";
/// The line Claude prints in place of the browser: the same authorize URL, with the paste route's
/// `redirect_uri`.
const CLAUDE_MANUAL_LINE: &str = "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?redirect_uri=https://platform.claude.com/oauth/code/callback";
/// The line Claude prints before it reads a pasted code.
const CLAUDE_PASTE_PROMPT: &str = "Paste code here if prompted >";
/// The line Claude's login ends with when it succeeds.
const CLAUDE_SUCCESS: &str = "Login successful.";
/// The line `FAKE_SIGNIN=cancel` prints, for either CLI.
const CANCEL_LINE: &str = "OAuth error: access_denied";
/// The line Codex's browser login prints; the callback it listens on is never bound.
const CODEX_CALLBACK_LINE: &str =
    "Starting local login server on http://127.0.0.1:1455/auth/callback";
/// The line `login --device-auth` prints: the device URL and the code.
const CODEX_DEVICE_LINE: &str =
    "To sign in, open https://auth.openai.com/codex/device and enter the code ABCD-EFGH";
/// The line `login --device-auth` prints under `FAKE_SIGNIN=device-off`: Codex's own refusal.
const CODEX_DEVICE_REFUSAL: &str = "Enable device code sign-in for Codex, Excel, PowerPoint, and Word in ChatGPT Security Settings, then run codex login --device-auth again.";
/// The line the browser login prints under `FAKE_SIGNIN=port-busy`.
const CODEX_PORT_BUSY_LINE: &str =
    "Port 1455 is already in use: another login is listening on 127.0.0.1:1455.";
/// The line a Codex login prints when it succeeds.
const CODEX_SUCCESS: &str = "Successfully logged in";
/// The line `codex login status` prints when the marker is there.
const CODEX_STATUS_IN: &str = "Logged in using ChatGPT";
/// The line `codex login status` prints when the marker is not there.
const CODEX_STATUS_OUT: &str = "Not logged in";
/// The line `codex logout` prints.
const CODEX_LOGOUT_LINE: &str = "Successfully logged out";
/// The line `auth logout` prints.
const CLAUDE_LOGOUT_LINE: &str = "Successfully logged out from your Claude account.";

/// The usage line for `claude-cli`, on stderr with exit 2.
const CLAUDE_USAGE: &str = "claude-cli takes auth login [--claudeai|--console], auth status [--json] and auth logout, with an optional leading --cli";
/// The usage line for `codex.js`, on stderr with exit 2.
const CODEX_USAGE: &str =
    "codex.js takes login, login --device-auth, login status, logout and app-server";

/// Which CLI a script stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Claude Code, from a script named `claude-cli`.
    Claude,
    /// Codex, from a script named `codex.js`.
    Codex,
}

/// The mode the script `argv[1]` names, by its file name in any folder, or `None` when it is
/// another program (then the normal command line runs).
pub(crate) fn mode(script: &OsStr) -> Option<Mode> {
    match Path::new(script).file_name()?.to_str()? {
        "claude-cli" => Some(Mode::Claude),
        "codex.js" => Some(Mode::Codex),
        _ => None,
    }
}

/// What `FAKE_SIGNIN` makes a sign-in do. Unset, empty and any value not named here mean
/// [`Script::OkAfter`] with 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Script {
    /// The login writes the marker after this many milliseconds, then exits 0.
    OkAfter(u64),
    /// The login prints `OAuth error: access_denied` and exits 1.
    Cancel,
    /// The login waits until it is killed.
    Never,
    /// The login reads one line from stdin: `FAKE-CODE` logs in, anything else exits 1.
    Paste,
    /// Codex's `login --device-auth` works. The default, as for every other value.
    Device,
    /// Codex's `login --device-auth` refuses with Codex's own message, exit 1. The browser login
    /// is not affected.
    DeviceOff,
    /// Codex's browser `login` finds port 1455 taken and exits 1 before it logs in.
    PortBusy,
    /// Plants the marker with this `authMethod` at startup, for any subcommand. The login then
    /// behaves as [`Script::OkAfter`] with 0.
    Reuse(String),
    /// Claude ignores `CLAUDE_SECURESTORAGE_CONFIG_DIR`, as a later Claude Code that dropped the
    /// variable would: a reused session's status then looks in `CLAUDE_CONFIG_DIR` (the
    /// run-time fallback's case). Otherwise as [`Script::OkAfter`] with 0.
    SecureIgnored,
}

impl Script {
    /// The script `value` (the `FAKE_SIGNIN` text) names.
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "cancel" => Self::Cancel,
            "never" => Self::Never,
            "paste" => Self::Paste,
            "device" => Self::Device,
            "device-off" => Self::DeviceOff,
            "port-busy" => Self::PortBusy,
            "secure-ignored" => Self::SecureIgnored,
            _ => value
                .strip_prefix("reuse:")
                .filter(|method| !method.is_empty())
                .map(|method| Self::Reuse(method.to_owned()))
                .or_else(|| {
                    value
                        .strip_prefix("ok-after:")?
                        .parse()
                        .ok()
                        .map(Self::OkAfter)
                })
                .unwrap_or(Self::OkAfter(0)),
        }
    }
}

/// Runs the sign-in `mode` with `args`, the arguments after the script, and returns the exit code.
pub(crate) fn run(mode: Mode, args: &[OsString]) -> i32 {
    session(mode, args).unwrap_or_else(|error| {
        stderr_write(&diagnostic(format_args!("{error}")));
        FAILED
    })
}

/// One run: the environment's settings, then the command. An error is a failed write to stdout
/// or stdin, or a missing home ([`user_home`]); a bad command is [`usage`].
fn session(mode: Mode, args: &[OsString]) -> io::Result<i32> {
    write_env_names()?;
    let script = Script::parse(&std::env::var("FAKE_SIGNIN").unwrap_or_default());
    let Some(home) = user_home(|name| std::env::var_os(name), cfg!(windows)) else {
        stderr_write(&diagnostic(if cfg!(windows) {
            "USERPROFILE and HOME are not set"
        } else {
            "HOME is not set"
        }));
        return Ok(FAILED);
    };
    let folder = match mode {
        Mode::Claude => claude_home(
            std::env::var_os("CLAUDE_SECURESTORAGE_CONFIG_DIR")
                .filter(|_| script != Script::SecureIgnored),
            std::env::var_os("CLAUDE_CONFIG_DIR"),
            &home,
        ),
        Mode::Codex => codex_home(std::env::var_os("CODEX_HOME"), &home),
    };
    if let Script::Reuse(method) = &script {
        plant(&folder, method)?;
    }
    let Some(args) = args
        .iter()
        .map(|arg| arg.to_str())
        .collect::<Option<Vec<&str>>>()
    else {
        return Ok(usage(mode));
    };
    match mode {
        Mode::Claude => claude(&args, &script, &folder),
        Mode::Codex => codex(&args, &script, &folder),
    }
}

/// Writes the usage line for `mode` to stderr and returns [`EXIT_LOAD`].
fn usage(mode: Mode) -> i32 {
    stderr_write(&diagnostic(match mode {
        Mode::Claude => CLAUDE_USAGE,
        Mode::Codex => CODEX_USAGE,
    }));
    EXIT_LOAD
}

/// The user's home folder, where the real CLIs find `.codex` and `.claude`, from `var`, the
/// environment's lookup: on Windows (`windows`) `USERPROFILE` as Codex and Claude Code read it,
/// else `HOME` when `USERPROFILE` is unset or empty; elsewhere `HOME`.
pub(crate) fn user_home(var: impl Fn(&str) -> Option<OsString>, windows: bool) -> Option<PathBuf> {
    if windows {
        var("USERPROFILE")
            .filter(|home| !home.is_empty())
            .or_else(|| var("HOME").filter(|home| !home.is_empty()))
            .map(PathBuf::from)
    } else {
        var("HOME").map(PathBuf::from)
    }
}

/// Where Claude Code keeps its login: `CLAUDE_SECURESTORAGE_CONFIG_DIR` when set (`""` is
/// `$HOME/.claude`), else `CLAUDE_CONFIG_DIR` when set and not empty, else `$HOME/.claude`.
pub(crate) fn claude_home(
    secure: Option<OsString>,
    config: Option<OsString>,
    home: &Path,
) -> PathBuf {
    let default = home.join(".claude");
    match (secure, config) {
        (Some(secure), _) if !secure.is_empty() => PathBuf::from(secure),
        (None, Some(config)) if !config.is_empty() => PathBuf::from(config),
        _ => default,
    }
}

/// Where Codex keeps its login: `CODEX_HOME` when set and not empty, else `$HOME/.codex`.
pub(crate) fn codex_home(codex: Option<OsString>, home: &Path) -> PathBuf {
    match codex {
        Some(folder) if !folder.is_empty() => PathBuf::from(folder),
        _ => home.join(".codex"),
    }
}

/// Writes the names of this process's environment variables, sorted and one per line, to the
/// path `FAKE_SIGNIN_ENV_OUT` names. Names only: a test can check which variables reached the
/// CLI without a value leaking into its output.
fn write_env_names() -> io::Result<()> {
    let Some(path) = std::env::var_os("FAKE_SIGNIN_ENV_OUT").filter(|path| !path.is_empty()) else {
        return Ok(());
    };
    let mut names: Vec<String> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut text = String::new();
    for name in &names {
        text.push_str(name);
        text.push('\n');
    }
    write::write(path, text)
}

/// The Claude commands: `auth login` (`--claudeai` or `--console`), `auth status [--json]` and
/// `auth logout`, after an optional leading `--cli`.
fn claude(args: &[&str], script: &Script, folder: &Path) -> io::Result<i32> {
    let args = args.strip_prefix(&["--cli"]).unwrap_or(args);
    match args {
        ["auth", "login"] | ["auth", "login", "--claudeai" | "--console"] => {
            claude_login(script, folder)
        }
        ["auth", "status"] | ["auth", "status", "--json"] => claude_status(folder),
        ["auth", "logout"] => logout(folder, CLAUDE_LOGOUT_LINE),
        _ => Ok(usage(Mode::Claude)),
    }
}

/// Claude's login: calls `$BROWSER` (when set) with the authorize URL, prints the manual route and
/// the paste prompt, then ends as the script says.
fn claude_login(script: &Script, folder: &Path) -> io::Result<i32> {
    if let Some(browser) = std::env::var_os("BROWSER").filter(|browser| !browser.is_empty()) {
        open_browser(&browser, CLAUDE_BROWSER_URL);
    }
    say(CLAUDE_MANUAL_LINE)?;
    say(CLAUDE_PASTE_PROMPT)?;
    complete(script, || plant(folder, "claude.ai"), CLAUDE_SUCCESS)
}

/// Runs `browser url` and waits for it. Its exit code is ignored, as the real CLI ignores it, and
/// its stdout goes nowhere, so the lines this mode prints stay the only ones on stdout.
#[expect(
    clippy::disallowed_methods,
    reason = "Claude's login runs the BROWSER command it is given and waits for it, as the real CLI does; no other child starts"
)]
fn open_browser(browser: &OsStr, url: &str) {
    let started = std::process::Command::new(browser)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status();
    if let Err(error) = started {
        stderr_write(&diagnostic(format_args!(
            "BROWSER could not start: {error}"
        )));
    }
}

/// `auth status --json`: the signed-in line when the marker is there, else the signed-out line.
/// Both are printed exactly as the brief has them, with the keys in that order. The email is the
/// marker's, `test@example.com` unless a test planted another.
fn claude_status(folder: &Path) -> io::Result<i32> {
    let line = match marker(folder) {
        Some((method, email)) => format!(
            r#"{{"loggedIn":true,"authMethod":{},"apiProvider":"firstParty","email":{},"subscriptionType":"max"}}"#,
            json!(method),
            json!(email)
        ),
        None => r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#.to_owned(),
    };
    say(&line)?;
    Ok(EXIT_OK)
}

/// The Codex commands: `login`, `login --device-auth`, `login status`, `logout` and `app-server`,
/// each after any number of leading `-c <key=value>` config overrides, which are skipped as
/// Codex takes them before its subcommand (`-c cli_auth_credentials_store="file"`).
fn codex(args: &[&str], script: &Script, folder: &Path) -> io::Result<i32> {
    let mut args = args;
    while let ["-c", _, rest @ ..] = args {
        args = rest;
    }
    match args {
        ["login", "status"] => codex_status(folder),
        ["login"] => codex_browser_login(script, folder),
        ["login", "--device-auth"] => codex_device_login(script, folder),
        ["logout"] => logout(folder, CODEX_LOGOUT_LINE),
        ["app-server"] => app_server(folder),
        _ => Ok(usage(Mode::Codex)),
    }
}

/// Codex's browser login. It prints where its callback listens, and binds nothing there.
fn codex_browser_login(script: &Script, folder: &Path) -> io::Result<i32> {
    if matches!(script, Script::PortBusy) {
        say(CODEX_PORT_BUSY_LINE)?;
        return Ok(FAILED);
    }
    say(CODEX_CALLBACK_LINE)?;
    complete(script, || plant(folder, "chatgpt"), CODEX_SUCCESS)
}

/// Codex's `login --device-auth`: prints the device URL and code, unless `device-off` refuses it.
fn codex_device_login(script: &Script, folder: &Path) -> io::Result<i32> {
    if matches!(script, Script::DeviceOff) {
        say(CODEX_DEVICE_REFUSAL)?;
        return Ok(FAILED);
    }
    say(CODEX_DEVICE_LINE)?;
    complete(script, || plant(folder, "chatgpt"), CODEX_SUCCESS)
}

/// `login status`: exit 0 with the signed-in line when the marker is there, else exit 1.
fn codex_status(folder: &Path) -> io::Result<i32> {
    if marker_method(folder).is_some() {
        say(CODEX_STATUS_IN)?;
        Ok(EXIT_OK)
    } else {
        say(CODEX_STATUS_OUT)?;
        Ok(FAILED)
    }
}

/// `app-server`: answers each JSON-RPC line on stdin with one line on stdout, until stdin closes.
fn app_server(folder: &Path) -> io::Result<i32> {
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = reply(&line, folder) {
            say(&reply)?;
        }
    }
    Ok(EXIT_OK)
}

/// The reply to one client line, as JSON text, or `None` for a notification (a message with no
/// `id`). `initialize` and `account/read` answer as Codex's schema says; any other method is an
/// error.
fn reply(line: &str, folder: &Path) -> Option<String> {
    let request: Value = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {error}")}})
                    .to_string(),
            );
        }
    };
    let id = request.get("id")?.clone();
    let method = request["method"].as_str().unwrap_or_default();
    let reply = match method {
        "initialize" => json!({"jsonrpc": "2.0", "id": id, "result": {
            "codexHome": folder.to_string_lossy(),
            "platformFamily": if cfg!(windows) { "windows" } else { "unix" },
            "platformOs": std::env::consts::OS,
            "userAgent": "fake_agent codex-cli",
        }}),
        "account/read" => json!({"jsonrpc": "2.0", "id": id, "result": account_read(folder)}),
        _ => json!({"jsonrpc": "2.0", "id": id, "error": {
            "code": -32601, "message": format!("method not found: {method}"),
        }}),
    };
    Some(reply.to_string())
}

/// `account/read`'s result. With the marker in `folder`, the `ChatGPT` account named by the
/// marker's email (`test@example.com` unless a test planted another, as R17's swap tests do) on
/// `plus`, or, when the marker's `authMethod` is `apikey`, an API-key account, which names no
/// email. Without it, the signed-out reply: `account` null and `requiresOpenaiAuth` true.
fn account_read(folder: &Path) -> Value {
    match marker(folder) {
        Some((method, _)) if method == API_KEY_METHOD => {
            json!({"account": {"type": "apiKey"}, "requiresOpenaiAuth": true})
        }
        Some((_, email)) => json!({
            "account": {"type": "chatgpt", "email": email, "planType": "plus"},
            "requiresOpenaiAuth": false,
        }),
        None => json!({"account": null, "requiresOpenaiAuth": true}),
    }
}

/// `auth logout` and `logout`: removes the marker when there is one, and prints `message`.
fn logout(folder: &Path, message: &str) -> io::Result<i32> {
    let marker = folder.join(MARKER);
    if marker.is_file() {
        write::remove_file(&marker)?;
    }
    say(message)?;
    Ok(EXIT_OK)
}

/// The end of every login, after its lines are printed. `FAKE_SIGNIN` decides it: a cancel, a
/// wait that never ends, a pasted code, or the marker after the script's delay.
fn complete(
    script: &Script,
    done: impl FnOnce() -> io::Result<()>,
    success: &str,
) -> io::Result<i32> {
    match script {
        Script::Cancel => {
            say(CANCEL_LINE)?;
            Ok(FAILED)
        }
        Script::Never => park_forever(),
        Script::Paste => {
            if read_line()? == PASTE_CODE {
                done()?;
                say(success)?;
                Ok(EXIT_OK)
            } else {
                say("Login failed: the code is not FAKE-CODE.")?;
                Ok(FAILED)
            }
        }
        _ => {
            let ms = if let Script::OkAfter(ms) = script {
                *ms
            } else {
                0
            };
            if ms > 0 {
                crate::run::effects::pause(Duration::from_millis(ms));
            }
            done()?;
            say(success)?;
            Ok(EXIT_OK)
        }
    }
}

/// One line from stdin, without its line ending. At end of input it is empty.
fn read_line() -> io::Result<String> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// The `authMethod` the marker in `folder` holds, or `None` when there is no marker.
fn marker_method(folder: &Path) -> Option<String> {
    marker(folder).map(|(method, _)| method)
}

/// The marker in `folder`: its `authMethod` and its `email` (`test@example.com` when it
/// names none), or `None` when there is none. For Codex a file named [`CODEX_LOGIN_FILE`]
/// counts as the marker too, so a test can stand an empty file of that name in `~/.codex` and
/// the engine's hard link of it into `CODEX_HOME` signs the home in, as Codex's own file would.
/// The file is the test's dummy, never a real credential.
fn marker(folder: &Path) -> Option<(String, String)> {
    let path = [MARKER, CODEX_LOGIN_FILE]
        .into_iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())?;
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let marker = serde_json::from_str::<Value>(&text).unwrap_or_default();
    let field = |name: &str| marker[name].as_str().map(str::to_owned);
    Some((
        field("authMethod").unwrap_or_default(),
        field("email").unwrap_or_else(|| EMAIL.to_owned()),
    ))
}

/// Writes the marker in `folder` for `auth_method`, creating the folder first.
fn plant(folder: &Path, auth_method: &str) -> io::Result<()> {
    write::create_dir_all(folder)?;
    let marker = json!({"authMethod": auth_method, "email": EMAIL});
    write::write(folder.join(MARKER), format!("{marker}\n"))
}

/// Writes `line` and a newline to stdout, then flushes, because the client reads each line as it
/// comes.
fn say(line: &str) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(line.as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

/// Writes `text` to stderr. A failed write is ignored: diagnostics are best effort.
fn stderr_write(text: &str) {
    let mut stderr = io::stderr().lock();
    stderr.write_all(text.as_bytes()).unwrap_or_default();
}

#[cfg(test)]
mod tests;
