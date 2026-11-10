//! A session's `PATH` inside the agent's own shell commands, on macOS and Linux (decision record
//! 0048, `### PATH`). The session process gets stetdoc's, the shell browser's and the Python
//! runtime's folders first on its `PATH` (`agent::launch_spec`), but neither agent runs its
//! commands with that `PATH` as it is: each runs them through the account's login shell, and
//! macOS's (`/etc/zprofile` → `path_helper`) puts `/usr/bin` first again, so `python3` was
//! `/usr/bin/python3` and `import docx` failed.
//!
//! - Codex (`@openai/codex` 0.162.1) runs a command with `-lc` unless `allow_login_shell` is
//!   false (`config/src/config_toml.rs:221`, default true at `core/src/config/mod.rs:3852`,
//!   `core/src/shell.rs:25`), and with `shell_snapshot` (on by default) sources a login-shell
//!   snapshot first, then re-exports `shell_environment_policy.set`'s variables
//!   (`core/src/tools/runtimes/mod.rs`, `maybe_wrap_shell_lc_with_snapshot`). The session's
//!   `CODEX_CONFIG` gets both keys ([`super::codex::with_shell_path`]), `set.PATH` being the
//!   session's `PATH`; on macOS the folders `path_helper` would add ([`path_helper_entries`])
//!   go right after Clauseo's ([`mac_shell_path`]), so nothing a lawyer's GUI or login `PATH`
//!   had is dropped. `CODEX_CONFIG` is the session's own (codex-acp sends it as each thread's
//!   `config`, which app-server turns into overrides on `config.toml`,
//!   `app-server/src/config_manager.rs:446-457`), so two sessions with different folders never
//!   share it, as they would the home's `config.toml`.
//! - Claude Code (2.1.293) sources a login-shell snapshot before each Bash command, then the
//!   file `CLAUDE_ENV_FILE` names. The launch writes [`ENV_FILE`] in the session's own run
//!   folder, one line putting the folders first on the `PATH` the snapshot built (so it keeps
//!   everything there), and sets [`ENV_FILE_VAR`] to it.
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
}

/// [`SessionPath`] for an agent of `kind` with `dirs` first on its `PATH`, its file written in
/// `run_root`. File system calls: off the actor.
pub(crate) fn prepare(
    dirs: Vec<PathBuf>,
    hook_path: bool,
    kind: Option<AgentKind>,
    run_root: &Path,
) -> SessionPath {
    prepare_on(dirs, hook_path, kind, run_root, Os::host(), Path::new(ETC))
}

/// [`prepare`] on `os`, with `path_helper`'s files under `etc`.
pub(crate) fn prepare_on(
    dirs: Vec<PathBuf>,
    hook_path: bool,
    kind: Option<AgentKind>,
    run_root: &Path,
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
        Some(AgentKind::Claude) => path.env_file = write_env_file(run_root, &path.dirs),
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

/// The one line of [`ENV_FILE`]: `export PATH='<dir1>:<dir2>…':"$PATH"`, each folder inside
/// the single quotes with a `'` written as `'\''`. A folder that holds `:`, a line break or is
/// not UTF-8 is left out (counted in a warning); none left is `None`.
pub(crate) fn env_file_text(dirs: &[PathBuf]) -> Option<String> {
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

/// Writes [`env_file_text`] of `dirs` to `<run_root>/clauseo-env.sh` and gives its path; with
/// no line, removes a file left there and gives `None`, as it does when the write fails.
fn write_env_file(run_root: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    let file = run_root.join(ENV_FILE);
    let Some(text) = env_file_text(dirs) else {
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
/// `allow_login_shell = false` and `shell_environment_policy.set.PATH` (`env`'s `PATH`, on
/// macOS through [`mac_shell_path`]); a Claude session with its file gets [`ENV_FILE_VAR`],
/// over any inherited one. Otherwise `env` is left as it is, an inherited `CLAUDE_ENV_FILE`
/// included.
pub(crate) fn apply_on(env: &mut BTreeMap<OsString, OsString>, path: &SessionPath, os: Os) {
    if os == Os::Windows || path.dirs.is_empty() {
        return;
    }
    if let Some(file) = &path.env_file {
        env.retain(|name, _| !name.eq_ignore_ascii_case(ENV_FILE_VAR));
        env.insert(OsString::from(ENV_FILE_VAR), file.as_os_str().to_owned());
    }
    if path.codex {
        codex_keys(env, path, os);
    }
}

/// The Codex part of [`apply_on`]. A `PATH` or `CODEX_CONFIG` that is not UTF-8 or JSON leaves
/// the config as it is, with a warning.
fn codex_keys(env: &mut BTreeMap<OsString, OsString>, path: &SessionPath, os: Os) {
    let value = |name: &str| {
        env.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| value.to_str())
            .map(str::to_owned)
    };
    let (Some(session), Some(config)) = (value("PATH"), value(super::codex::CONFIG_VAR)) else {
        tracing::warn!("Codex's shell keeps the login PATH: no UTF-8 PATH or CODEX_CONFIG");
        return;
    };
    let shell = match os {
        Os::Mac => mac_shell_path(&path.dirs, &path.etc_paths, &session),
        Os::Linux | Os::Windows => session,
    };
    let Ok(mut config) = serde_json::from_str::<serde_json::Map<_, _>>(&config) else {
        tracing::warn!("Codex's shell keeps the login PATH: CODEX_CONFIG is not an object");
        return;
    };
    super::codex::with_shell_path(&mut config, &shell);
    tracing::debug!(
        folders = path.dirs.len(),
        helper = path.etc_paths.len(),
        "Codex's shell runs without a login shell, with the session's PATH"
    );
    env.insert(
        OsString::from(super::codex::CONFIG_VAR),
        OsString::from(serde_json::Value::Object(config).to_string()),
    );
}

#[cfg(test)]
mod tests;
