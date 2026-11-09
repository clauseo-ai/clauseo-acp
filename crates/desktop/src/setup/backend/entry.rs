//! What the backend makes from an install: its `agents.<id>` entry ([`launch`]) and its state
//! on the wire ([`progress`]).

use std::collections::BTreeMap;
use std::path::Path;

use clauseo_agents::{InstallState, Installed};
use clauseo_protocol::{
    AgentId, AgentKind, Count, ErrorCode, InstallLog, InstallProgress, SignInSource,
};
use clauseo_settings::AgentLaunch;

/// The `agents.<id>` entry for `installed` (decision record 0048, "Installed becomes
/// launchable"): `command` the Node, `args` the adapter's script, and the env: for Claude
/// `ANTHROPIC_API_KEY` empty, `CLAUDE_CONFIG_DIR` the home and `CLAUDE_SECURESTORAGE_CONFIG_DIR`
/// empty for a `Reused` login or the home for `Own`; for Codex `CODEX_API_KEY` empty and, on
/// macOS and Linux, `HOME` the home. `None` when a path is not UTF-8 (settings hold text).
pub(crate) fn launch(
    agent: &AgentId,
    installed: &Installed,
    home: &Path,
    source: SignInSource,
) -> Option<AgentLaunch> {
    let command = installed.node.to_str()?.to_owned();
    let script = installed.script.to_str()?.to_owned();
    let home = home.to_str()?.to_owned();
    let mut env = BTreeMap::new();
    match AgentKind::of(agent) {
        Some(AgentKind::Claude) => {
            env.insert("ANTHROPIC_API_KEY".to_owned(), String::new());
            env.insert("CLAUDE_CONFIG_DIR".to_owned(), home.clone());
            let storage = match source {
                SignInSource::Reused => String::new(),
                SignInSource::Own => home,
            };
            env.insert("CLAUDE_SECURESTORAGE_CONFIG_DIR".to_owned(), storage);
        }
        Some(AgentKind::Codex) => {
            env.insert("CODEX_API_KEY".to_owned(), String::new());
            if cfg!(not(windows)) {
                env.insert("HOME".to_owned(), home);
            }
        }
        None => {}
    }
    Some(AgentLaunch {
        command,
        args: vec![script],
        env,
    })
}

/// `state` on the wire, with `hosts` for a waiting or failed install; `None` for a state this
/// version does not know. A `Missing` says `installed_before` as given: whether Clauseo's
/// `agents.<id>` entry is still in the settings (`refresh::installed_before`, #859).
pub(crate) fn progress(
    state: &InstallState,
    hosts: &[String],
    installed_before: bool,
) -> Option<InstallProgress> {
    Some(match state {
        InstallState::Missing { download_bytes } => InstallProgress::Missing {
            download_bytes: Count::clamped(*download_bytes),
            installed_before,
        },
        InstallState::Waiting { reason, log } => InstallProgress::Waiting {
            code: ErrorCode::from_str_lossy(reason.code()),
            hosts: hosts.to_vec(),
            log: InstallLog::new(log.lines().iter().cloned()),
        },
        InstallState::Downloading { received, total } => InstallProgress::Downloading {
            received: Count::clamped(*received),
            total: Count::clamped(*total),
        },
        InstallState::Installing => InstallProgress::Installing,
        InstallState::Ready { version } => InstallProgress::Ready {
            version: version.clone(),
        },
        InstallState::Failed(failure) => InstallProgress::Failed {
            code: ErrorCode::from_str_lossy(failure.kind.code()),
            need_mb: failure.need_mb.map(Count::clamped),
            free_mb: failure.free_mb.map(Count::clamped),
            hosts: hosts.to_vec(),
            log: InstallLog::new(failure.log.lines().iter().cloned()),
        },
        InstallState::Paused => InstallProgress::Paused,
        InstallState::Managed => InstallProgress::Managed,
        _ => return None,
    })
}

/// Logs `state` published to the windows, every state but the download's ticks: with the
/// agents' own line as each is sent, the log shows any lag between the job and the windows.
/// On the lifecycle target, as host.log keeps INFO only from `clauseo_*::lifecycle`.
pub(super) fn log_published(agent: &AgentId, state: &InstallState) {
    if let Some(word) = published_word(state) {
        tracing::info!(
            target: "clauseo_desktop::lifecycle",
            %agent,
            state = word,
            "install state published to the windows"
        );
    }
}

/// The word for an install state's publish line; `None` for `Downloading`, which ticks every
/// 250 ms.
fn published_word(state: &InstallState) -> Option<&'static str> {
    match state {
        InstallState::Downloading { .. } => None,
        InstallState::Installing => Some("installing"),
        InstallState::Ready { .. } => Some("ready"),
        InstallState::Failed(failure) => Some(failure.kind.word()),
        InstallState::Waiting { .. } => Some("waiting"),
        _ => Some("other"),
    }
}
