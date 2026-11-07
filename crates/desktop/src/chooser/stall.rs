//! Whether Windows' own folder dialog would stall ([`Probes::would_stall`]): it blocks about 21 s
//! inside `IFileDialog::Show()` when a known folder it shows (Documents, the Libraries in Roaming
//! `AppData`, ...) sits on a share that is down, and nothing in the dialog's options avoids that.
//! So before drawing it, the known folders' paths are read without touching the disk, and each
//! network server among them is probed once, all within one shared deadline. Plain Rust behind
//! [`KnownFolders`], so the timing is tested on every OS.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

/// How long the probes of all servers together may take.
pub(crate) const PROBE_WAIT: Duration = Duration::from_millis(1500);

/// The variable support can set to choose the picker: `app` (the chooser) or `native`.
pub(crate) const PICKER_VAR: &str = "CLAUSEO_FOLDER_PICKER";

/// What the pre-check reads on the machine.
pub(crate) trait KnownFolders: Send + Sync + 'static {
    /// The known folders' paths, unchecked: Documents, Roaming `AppData`, Desktop, Downloads,
    /// Pictures, Music and Videos.
    fn paths(&self) -> Vec<String>;
    /// Whether the drive `letter` is a network drive.
    fn remote_drive(&self, letter: char) -> bool;
    /// Whether `path` answers (Windows reads its attributes). May block.
    fn answers(&self, path: &str) -> bool;
}

/// Which picker a pick opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// The OS's folder dialog.
    Native,
    /// The in-app chooser.
    Chooser,
}

/// The picker for a pick: the native dialog off Windows, whatever `setting` says; on Windows
/// `setting`'s choice ([`PICKER_VAR`]), else the chooser when `stalls` says the dialog would.
pub(crate) fn route(
    on_windows: bool,
    setting: Option<&str>,
    stalls: impl FnOnce() -> bool,
) -> Route {
    if !on_windows {
        return Route::Native;
    }
    match setting {
        Some("app") => Route::Chooser,
        Some("native") => Route::Native,
        _ if stalls() => Route::Chooser,
        _ => Route::Native,
    }
}

/// The servers whose probe is still running, from this pick or an earlier one.
#[derive(Default)]
pub(crate) struct Probes {
    running: Arc<Mutex<HashSet<String>>>,
}

impl Probes {
    /// Whether the native dialog would stall: a known folder on the network whose server does
    /// not answer within `wait`, answers with an error, or is still stuck on an earlier probe
    /// (which gets no second thread). Reads no disk when no known folder is on the network.
    /// Logs the outcome only, never a path.
    pub(crate) fn would_stall(&self, known: &Arc<dyn KnownFolders>, wait: Duration) -> bool {
        let deadline = crate::open::now() + wait;
        let servers = servers(known.as_ref());
        if servers.is_empty() {
            return false;
        }
        let (send, answers) = mpsc::channel();
        let mut waiting = 0_usize;
        for (server, path) in servers {
            if !lock(&self.running).insert(server.clone()) {
                tracing::info!("folder picker: a network folder's earlier probe is still stuck");
                return true;
            }
            let running = Arc::clone(&self.running);
            let known = Arc::clone(known);
            let send = send.clone();
            let spawned = std::thread::Builder::new()
                .name("clauseo-picker-probe".into())
                .spawn(move || {
                    let answered = known.answers(&path);
                    lock(&running).remove(&server);
                    // Gone once the deadline passed; the answer is then dropped.
                    let _gone = send.send(answered);
                });
            if spawned.is_err() {
                return true;
            }
            waiting += 1;
        }
        for _ in 0..waiting {
            let left = deadline.saturating_duration_since(crate::open::now());
            if answers.recv_timeout(left) != Ok(true) {
                tracing::info!("folder picker: a network folder does not answer");
                return true;
            }
        }
        tracing::info!("folder picker: the network folders answer");
        false
    }
}

/// The known folders on the network, one per server: the server's name (lower case, or the
/// drive, `x:`) and the first known folder on it.
fn servers(known: &dyn KnownFolders) -> HashMap<String, String> {
    let mut servers = HashMap::new();
    for path in known.paths() {
        let unc = path.replace('/', "\\");
        let server = if let Some(rest) = unc.strip_prefix("\\\\") {
            rest.split('\\')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_lowercase)
        } else {
            let mut chars = path.chars();
            match (chars.next(), chars.next()) {
                (Some(letter), Some(':'))
                    if letter.is_ascii_alphabetic() && known.remote_drive(letter) =>
                {
                    Some(format!("{}:", letter.to_ascii_lowercase()))
                }
                _ => None,
            }
        };
        if let Some(server) = server {
            servers.entry(server).or_insert(path);
        }
    }
    servers
}

/// Locks `mutex`, past a poisoned lock: the set stays usable.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
