//! A session's `PATH` inside the agent's own shell commands, on macOS and Linux (decision record
//! 0048, `### PATH`). The session process gets stetdoc's, the shell browser's and the Python
//! runtime's folders first on its `PATH` (`agent::launch_spec`), but neither agent runs its
//! commands with that `PATH` as it is: each runs them through the account's login shell, and
//! macOS's (`/etc/zprofile` → `path_helper`) puts `/usr/bin` first again, so `python3` was
//! `/usr/bin/python3` and `import docx` failed.
//!
//! - Codex (`@openai/codex` 0.162.1, `rust-v0.162.1`) runs a command with `-lc` unless
//!   `allow_login_shell` is false (`config/src/config_toml.rs:221`, default true at
//!   `core/src/config/mod.rs:3852`, `core/src/tools/handlers/unified_exec.rs:104-113`), and
//!   with `shell_snapshot` (on by default) sources a login-shell snapshot first. The session's
//!   `CODEX_CONFIG` gets `allow_login_shell = false` and `features.shell_snapshot = false`
//!   ([`super::codex::without_login_shell`]), so a command runs `-c` in Codex's own
//!   environment, whose `PATH` is the session's with Codex's bundled tools' folder put first by
//!   Codex itself. On macOS the session's `PATH` also gets the folders `path_helper` would add
//!   ([`path_helper_entries`]) right after Clauseo's ([`mac_shell_path`]), so the GUI `PATH` and
//!   `/etc/paths` keep reaching Codex. The trade-off, on macOS and Linux alike: with no login
//!   shell and no snapshot, Codex's commands lose what the lawyer's own shell startup adds (a
//!   `~/.zprofile` or `~/.bash_profile` with `brew shellenv`, pyenv or nvm, a `~/.zshrc`'s
//!   aliases and exports). We accept it so Clauseo's tools (Python, stetdoc) and Codex's
//!   bundled `rg` come first. What reaches Codex is the `PATH` Clauseo builds: its folders,
//!   `path_helper`'s, then the rest of the `PATH` Clauseo was started with. Claude keeps the
//!   lawyer's startup files (its file only prepends). `CODEX_CONFIG` is the session's own
//!   (codex-acp sends it as each thread's `config`, which app-server turns into overrides on
//!   `config.toml`, `app-server/src/config_manager.rs:446-457`), so two sessions with different
//!   folders never share it, as they would the home's `config.toml`.
//! - Claude Code (2.1.293) sources a login-shell snapshot before each Bash command, then the
//!   file `CLAUDE_ENV_FILE` names. The launch writes [`ENV_FILE`] in the session's own run
//!   folder, one line putting the folders first on the `PATH` the snapshot built (so it keeps
//!   everything there), and sets [`ENV_FILE_VAR`] to it. It also gets [`SHELL_VAR`] set to
//!   [`BASH`], with folders or without: Claude Code otherwise runs its commands in the
//!   account's shell, zsh on a Mac, where Claude's bash-style commands go wrong (an unmatched
//!   `*.docx` is an error, `$var` is not split into words). On Linux only when `/bin/bash`
//!   is there (NixOS has none): elsewhere Claude Code picks its own shell as before.
//! - A Claude session on macOS whose sandbox is on gets the trust roots file
//!   (`agent::loads::tls_roots`): `SSL_CERT_FILE`, `CURL_CA_BUNDLE` and `REQUESTS_CA_BUNDLE` name
//!   it in the session's environment and again in [`ENV_FILE`], after the `PATH` line, so they
//!   hold in each Bash command whatever the login-shell snapshot set.
//!
//! The file system work ([`prepare`]) runs on the launch-prep thread; [`apply`] edits the
//! launch's merged environment on the actor. Windows is left as it was: no key, no file. Logs
//! hold counts, never a folder.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::AgentKind;

use crate::log_file::Cause;

/// The variable naming the file Claude Code pastes into each Bash command after its snapshot.
pub(crate) const ENV_FILE_VAR: &str = "CLAUDE_ENV_FILE";

/// The variable naming the shell Claude Code runs its Bash commands in.
pub(crate) const SHELL_VAR: &str = "CLAUDE_CODE_SHELL";

/// The shell a Claude session's commands run in, off Windows ([`SHELL_VAR`]).
pub(crate) const BASH: &str = "/bin/bash";

/// The name of that file in the session's run folder.
pub(crate) const ENV_FILE: &str = "clauseo-env.sh";

/// The folder `path_helper` reads `paths` and `paths.d` from.
const ETC: &str = "/etc";

/// The OS a launch's shell rules follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Os {
    /// Windows: nothing changes.
    Windows,
    /// macOS: login shells run `path_helper`.
    Mac,
    /// Linux and any other Unix.
    Linux,
}

impl Os {
    /// The OS the host runs on.
    pub(crate) const fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }
}

/// A session's `PATH` folders and what its shells get, found on the launch-prep thread.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionPath {
    /// The folders first on the session's `PATH`, in order: stetdoc's, the shell browser's
    /// tools, then the profile hook's (`actor/turn/agents/path.rs`).
    pub(crate) dirs: Vec<PathBuf>,
    /// Whether any of the hook's folders went on, which drops `agent::PYTHON_VARS`.
    pub(crate) hook_path: bool,
    /// Whether the agent is Codex.
    pub(crate) codex: bool,
    /// On macOS, for a Codex session with folders: what `path_helper` would add.
    pub(crate) etc_paths: Vec<PathBuf>,
    /// For a Claude session with folders, off Windows: the [`ENV_FILE`] written.
    pub(crate) env_file: Option<PathBuf>,
    /// For a Claude session, on macOS, and on Linux when it is there: [`BASH`].
    pub(crate) shell: Option<PathBuf>,
    /// For a Claude session off Windows: the trust roots file its CA variables name
    /// (`agent::loads::tls_roots`), if the launch prep wrote one.
    pub(crate) ca_file: Option<PathBuf>,
}

/// [`SessionPath`] for an agent of `kind` with `dirs` first on its `PATH`, its file written in
/// `run_root`, a Claude session's CA variables naming `ca_file`. File system calls: off the
/// actor.
pub(crate) fn prepare(
    dirs: Vec<PathBuf>,
    hook_path: bool,
    kind: Option<AgentKind>,
    run_root: &Path,
    ca_file: Option<&Path>,
) -> SessionPath {
    prepare_on(
        dirs,
        hook_path,
        kind,
        (run_root, ca_file),
        Os::host(),
        Path::new(ETC),
    )
}

/// [`prepare`] on `os`, with `path_helper`'s files under `etc`.
pub(crate) fn prepare_on(
    dirs: Vec<PathBuf>,
    hook_path: bool,
    kind: Option<AgentKind>,
    (run_root, ca_file): (&Path, Option<&Path>),
    os: Os,
    etc: &Path,
) -> SessionPath {
    let mut path = SessionPath {
        dirs,
        hook_path,
        codex: kind == Some(AgentKind::Codex),
        ..SessionPath::default()
    };
    if os == Os::Windows {
        return path;
    }
    match kind {
        Some(AgentKind::Codex) if os == Os::Mac && !path.dirs.is_empty() => {
            path.etc_paths = path_helper_entries(etc);
        }
        Some(AgentKind::Claude) => {
            path.ca_file = ca_file.map(Path::to_path_buf);
            path.env_file = write_env_file(run_root, &path.dirs, ca_file);
            let bash = Path::new(BASH);
            path.shell = (os == Os::Mac || bash.is_file()).then(|| bash.to_owned());
        }
        _ => {}
    }
    path
}

/// [`path_helper_entries`] of the real `/etc`, for the integration tests that check a macOS
/// Codex launch's `PATH` (`tests/host/python_path.rs`).
#[cfg(feature = "test-support")]
pub fn path_helper_folders() -> Vec<PathBuf> {
    path_helper_entries(Path::new(ETC))
}

/// The folders macOS's `path_helper` puts on a login shell's `PATH`: `<etc>/paths` line by
/// line, then each file in `<etc>/paths.d` in name order, without blank lines and `#`
/// comments. A file that cannot be read adds nothing; only counts are logged.
pub(crate) fn path_helper_entries(etc: &Path) -> Vec<PathBuf> {
    let mut files = vec![etc.join("paths")];
    let mut more: Vec<PathBuf> = std::fs::read_dir(etc.join("paths.d"))
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    more.sort();
    files.extend(more);
    let mut unread = 0_usize;
    let mut entries = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            unread += 1;
            continue;
        };
        let lines = text.lines().map(str::trim);
        entries.extend(
            lines
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(PathBuf::from),
        );
    }
    tracing::debug!(
        files = files.len(),
        unread,
        entries = entries.len(),
        "path_helper's folders read for Codex's shell"
    );
    entries
}

/// Codex's shell `PATH` on macOS: `dirs`, then `helper`, then the rest of `path` (the session's
/// whole `PATH`), each folder kept where it first comes.
pub(crate) fn mac_shell_path(dirs: &[PathBuf], helper: &[PathBuf], path: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let firsts = dirs.iter().chain(helper).filter_map(|dir| dir.to_str());
    for part in firsts.chain(path.split(':')) {
        if !part.is_empty() && !kept.contains(&part) {
            kept.push(part);
        }
    }
    kept.join(":")
}

/// The text of [`ENV_FILE`]: [`path_line`], then, with `ca_file`, one line exporting each of
/// `agent::loads::tls_roots::CA_VARS` as it, quoted the same way (left out when it holds a
/// line break or is not UTF-8). Neither is `None`.
pub(crate) fn env_file_text(dirs: &[PathBuf], ca_file: Option<&Path>) -> Option<String> {
    let ca = ca_file
        .and_then(Path::to_str)
        .filter(|file| !file.contains(['\n', '\r']))
        .map(|file| {
            let quoted = file.replace('\'', r"'\''");
            let exports: Vec<String> = crate::agent::loads::tls_roots::CA_VARS
                .iter()
                .map(|name| format!("{name}='{quoted}'"))
                .collect();
            format!("export {}\n", exports.join(" "))
        });
    match (path_line(dirs), ca) {
        (None, None) => None,
        (line, ca) => Some(format!(
            "{}{}",
            line.unwrap_or_default(),
            ca.unwrap_or_default()
        )),
    }
}

/// The `PATH` line: `export PATH='<dir1>:<dir2>…':"$PATH"`, each folder inside the single
/// quotes with a `'` written as `'\''`. A folder that holds `:`, a line break or is not UTF-8
/// is left out (counted in a warning); none left is `None`.
fn path_line(dirs: &[PathBuf]) -> Option<String> {
    let usable: Vec<&str> = dirs
        .iter()
        .filter_map(|dir| dir.to_str())
        .filter(|dir| !dir.contains([':', '\n', '\r']))
        .collect();
    if usable.len() < dirs.len() {
        tracing::warn!(
            left_out = dirs.len() - usable.len(),
            "folders left out of Claude's shell PATH file"
        );
    }
    if usable.is_empty() {
        return None;
    }
    let quoted: Vec<String> = usable
        .iter()
        .map(|dir| dir.replace('\'', r"'\''"))
        .collect();
    Some(format!("export PATH='{}':\"$PATH\"\n", quoted.join(":")))
}

/// Writes [`env_file_text`] to `<run_root>/clauseo-env.sh` and gives its path; with no line,
/// removes a file left there and gives `None`, as it does when the write fails.
fn write_env_file(run_root: &Path, dirs: &[PathBuf], ca_file: Option<&Path>) -> Option<PathBuf> {
    let file = run_root.join(ENV_FILE);
    let Some(text) = env_file_text(dirs, ca_file) else {
        if let Err(error) = crate::write::remove_old_file(&file) {
            tracing::warn!(error = %Cause(&error), "an old Claude shell PATH file stayed");
        }
        return None;
    };
    match crate::write::replace_private_file(&file, text.as_bytes()) {
        Ok(()) => Some(file),
        Err(error) => {
            tracing::warn!(
                error = %Cause(&error),
                "Claude's shell PATH file was not written; its shell keeps the login PATH"
            );
            None
        }
    }
}

/// Edits the launch's merged `env` for `path`'s shells on the host's OS ([`apply_on`]).
pub(crate) fn apply(env: &mut BTreeMap<OsString, OsString>, path: &SessionPath) {
    apply_on(env, path, Os::host());
}

/// [`apply`] on `os`. With folders, off Windows: a Codex session's `CODEX_CONFIG` gets
/// `allow_login_shell = false` and `features.shell_snapshot = false`, and on macOS its `PATH`
/// goes through [`mac_shell_path`]. Off Windows, a Claude session with its file gets
/// [`ENV_FILE_VAR`], over any inherited one; without, an inherited `CLAUDE_ENV_FILE` stays.
/// A Claude session with its shell gets [`SHELL_VAR`], and one with its trust roots file
/// each of `tls_roots::CA_VARS` naming it, each over any inherited one.
pub(crate) fn apply_on(env: &mut BTreeMap<OsString, OsString>, path: &SessionPath, os: Os) {
    if os == Os::Windows {
        return;
    }
    if let Some(shell) = &path.shell {
        env.retain(|name, _| !name.eq_ignore_ascii_case(SHELL_VAR));
        env.insert(OsString::from(SHELL_VAR), shell.as_os_str().to_owned());
    }
    if let Some(file) = &path.ca_file {
        for var in crate::agent::loads::tls_roots::CA_VARS {
            env.retain(|name, _| !name.eq_ignore_ascii_case(var));
            env.insert(OsString::from(var), file.as_os_str().to_owned());
        }
    }
    if let Some(file) = &path.env_file {
        env.retain(|name, _| !name.eq_ignore_ascii_case(ENV_FILE_VAR));
        env.insert(OsString::from(ENV_FILE_VAR), file.as_os_str().to_owned());
    }
    if path.dirs.is_empty() {
        return;
    }
    if path.codex {
        codex_keys(env, path, os);
    }
}

/// The Codex part of [`apply_on`]. A `PATH` that is not UTF-8 is left as it is, and a
/// `CODEX_CONFIG` that is not a JSON object too, each with a warning.
fn codex_keys(env: &mut BTreeMap<OsString, OsString>, path: &SessionPath, os: Os) {
    let find = |env: &BTreeMap<OsString, OsString>, name: &str| {
        env.keys()
            .find(|key| key.eq_ignore_ascii_case(name))
            .cloned()
    };
    if os == Os::Mac
        && let Some(name) = find(env, "PATH")
    {
        if let Some(session) = env.get(&name).and_then(|value| value.to_str()) {
            let shell = mac_shell_path(&path.dirs, &path.etc_paths, session);
            env.insert(name, OsString::from(shell));
        } else {
            tracing::warn!("path_helper's folders left off Codex's PATH: not UTF-8");
        }
    }
    let name = OsString::from(super::codex::CONFIG_VAR);
    let config = env.get(&name).and_then(|value| value.to_str());
    let Some(Ok(mut config)) = config.map(serde_json::from_str::<serde_json::Map<_, _>>) else {
        tracing::warn!("Codex's shell keeps its login shell: CODEX_CONFIG is not a JSON object");
        return;
    };
    super::codex::without_login_shell(&mut config);
    tracing::debug!(
        folders = path.dirs.len(),
        helper = path.etc_paths.len(),
        "Codex's shell runs without a login shell or snapshot, with the session's PATH"
    );
    env.insert(
        name,
        OsString::from(serde_json::Value::Object(config).to_string()),
    );
}

#[cfg(test)]
mod tests;
