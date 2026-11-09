//! What the backend makes from an install: its `agents.<id>` entry ([`launch`]) and its state
//! on the wire ([`progress`]), and the Python runtime's ([`python_event`]).

use std::collections::BTreeMap;
use std::path::Path;

use clauseo_agents::{
    FailureKind, InstallState, Installed, PythonBlocker, PythonFailure, PythonFailureKind,
    PythonState,
};
use clauseo_protocol::{
    AgentId, AgentKind, Count, ErrorCode, InstallLog, InstallProgress, PythonBlockedBy,
    PythonFailureCause, PythonProgress, SetupEvent, SignInSource,
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

/// Python's state out to the windows: `SetupInfo` again, with the keychain's answer as at the
/// start and `python` set. A state the wire does not know is not sent. First the launch hook's
/// Python folders follow it (`setup/python_path.rs`): `Ready` fills them, any other empties
/// them.
pub(crate) fn python_event(backend: &super::AgentsBackend, state: &PythonState) {
    backend.wiring.python.follow(state);
    let Some(python) = python_progress(state, &backend.wiring.python_hosts) else {
        return;
    };
    backend.events.publish(SetupEvent::SetupInfo {
        keys_available: backend.keys.available(),
        python: Some(python),
    });
}

/// `agents`' Python state as the wire's, with `hosts` (Python's download hosts) and the agent
/// install's code for a waiting or download-blocked one, as [`progress`] gives an agent's;
/// `None` for a state this version does not know.
pub(crate) fn python_progress(state: &PythonState, hosts: &[String]) -> Option<PythonProgress> {
    let log = |log: &clauseo_agents::InstallLog| InstallLog::new(log.lines().iter().cloned());
    Some(match state {
        PythonState::Unavailable => PythonProgress::Unavailable,
        PythonState::Missing { download_bytes } => PythonProgress::Missing {
            download_bytes: Count::clamped(*download_bytes),
        },
        PythonState::Waiting { reason, log: lines } => PythonProgress::Waiting {
            code: Some(ErrorCode::from_str_lossy(reason.code())),
            hosts: hosts.to_vec(),
            log: log(lines),
        },
        PythonState::Downloading { received, total } => PythonProgress::Downloading {
            received: Count::clamped(*received),
            total: Count::clamped(*total),
        },
        PythonState::Installing => PythonProgress::Installing,
        PythonState::Ready { version } => PythonProgress::Ready {
            version: version.clone(),
        },
        PythonState::Failed(failure) => python_failed(failure, hosts, log(&failure.log)),
        _ => return None,
    })
}

/// A failed Python install on the wire: a download-side failure with the agent install's code,
/// a blocked download with `hosts` too, a refused start with what refused it.
fn python_failed(failure: &PythonFailure, hosts: &[String], log: InstallLog) -> PythonProgress {
    let (cause, kind) = match failure.kind {
        PythonFailureKind::Blocked => (PythonFailureCause::Blocked, Some(FailureKind::Blocked)),
        PythonFailureKind::DiskFull => (PythonFailureCause::DiskFull, Some(FailureKind::DiskFull)),
        PythonFailureKind::RunBlocked => (PythonFailureCause::RunBlocked, None),
        PythonFailureKind::RunFailed => (PythonFailureCause::RunFailed, None),
        _ => (PythonFailureCause::Failed, Some(FailureKind::Failed)),
    };
    let blocked = cause == PythonFailureCause::Blocked;
    PythonProgress::Failed {
        cause,
        code: kind.map(|kind| ErrorCode::from_str_lossy(kind.code())),
        hosts: if blocked { hosts.to_vec() } else { Vec::new() },
        blocked_by: failure.blocked_by.map(|by| match by {
            PythonBlocker::AppControl => PythonBlockedBy::AppControl,
            PythonBlocker::AppLocker => PythonBlockedBy::AppLocker,
            _ => PythonBlockedBy::Unknown,
        }),
        log,
    }
}

/// `state` on the wire, with `hosts` for a waiting or failed install; `None` for a state this
/// version does not know.
pub(crate) fn progress(state: &InstallState, hosts: &[String]) -> Option<InstallProgress> {
    Some(match state {
        InstallState::Missing { download_bytes } => InstallProgress::Missing {
            download_bytes: Count::clamped(*download_bytes),
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
