//! A new matter folder's probe, off the actor. `CreateMatter` with a new folder (picked or in
//! the default location) needs its parent as the file system names it, so the registry check
//! compares it with canonical folders, and needs to know whether `<parent>/<name>` exists. Both
//! calls may hang for 20-80 s on a parent on a network share that does not answer (a Documents
//! folder redirected to a share, off the VPN), and the actor is the whole host's loop, so they
//! run on an [`off_actor`] thread, waited on for [`PROBE_WAIT`] on the host's clock. The answer
//! comes back as [`Input::NewFolder`]; a share that has not answered by then fails the command
//! with `matter.folder_missing` (the code for a folder whose drive is not mounted), naming no
//! path.
//!
//! While the probe runs the command stays in `actor.commands` with no matter, and nothing is
//! claimed: the folder is claimed when the answer comes back, on the actor, in
//! [`super::begin_create`], which checks it against the folders claimed then. So of two creates
//! of the same name probing at once, the one answered second finds the first's claim and is
//! refused. Shutdown does not wait for a probe (as for the Scratch scan, `scratch.rs`): an
//! answer that comes while the host stops fails its command as closed, and one that comes once
//! the actor is gone is dropped, its command having been answered closed with the actor.
//!
//! `LocateFile`'s folder check (`actor/locate.rs`) runs off the actor the same way, with the
//! same bound and the same refusal; its probe is in [`FolderProbes`] with this one.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_matters::{OsProbe, PlaceholderProbe};
use clauseo_protocol::{CommandId, ErrorCode};

use super::{Details, begin_create, not_begun};
use crate::actor::input::Input;
use crate::actor::{Actor, HostPhase};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::error::Error;
use crate::log_file::Cause;

/// How long the probe is waited for: a slow share that still answers has time to.
pub(crate) const PROBE_WAIT: Duration = Duration::from_secs(10);

/// The probe's [`off_actor`] job.
const PROBE_JOB: Job = Job {
    what: "new matter folder check",
    matter: None,
    wait: PROBE_WAIT,
};

/// The refusal for a probe that did not answer in time; it names no path.
pub(crate) const NO_ANSWER: &str =
    "the folder did not answer in time; check the network connection";

/// The probe: given a new folder's parent and name, the folder as it would be made (its parent
/// canonicalised, as written when that fails) and whether it exists. [`os_probe`], or a
/// stand-in in tests.
pub(crate) type Probe = Arc<dyn Fn(&Path, &str) -> (PathBuf, bool) + Send + Sync>;

/// The `LocateFile` probe (`actor/locate.rs`): given a file and its matter's folder, whether
/// the folder is there and whether the file lies in it once both resolve. [`probe_located`], or
/// a stand-in in tests.
pub(crate) type LocateProbe = Arc<dyn Fn(&Path, &Path) -> (bool, bool) + Send + Sync>;

/// The probes run off the actor: the host's own, or tests' stand-ins
/// (`HostConfig::with_folder_probes`).
#[derive(Clone)]
pub(crate) struct FolderProbes {
    /// A new matter's folder ([`probe_folder`]).
    pub(crate) new_folder: Probe,
    /// A located file's folder ([`probe_located`]).
    pub(crate) locate: LocateProbe,
    /// Whether a matter folder's file is a cloud-only placeholder, for every walk of the
    /// matter workers (`clauseo_matters::Settings::probe`): the OS's, or a test's stand-in
    /// (`HostConfig::with_placeholder_probe`).
    pub(crate) placeholder: Arc<dyn PlaceholderProbe + Send + Sync>,
    /// Tests: shutdown waits for the probes running too ([`stop_held`]).
    #[cfg(test)]
    stop_hold: Option<StopHold>,
}

impl FolderProbes {
    /// The probes `new_folder` and `locate`, as tests set them.
    #[cfg(test)]
    pub(crate) fn new(new_folder: Probe, locate: LocateProbe) -> Self {
        Self {
            new_folder,
            locate,
            placeholder: Arc::new(OsProbe),
            stop_hold: None,
        }
    }

    /// Shutdown waits for the probes running too, and says so on `told` each time it does, so a
    /// test can answer a probe while the host stops (as `HostConfig::with_scratch_stop_held`
    /// does for the Scratch scan).
    #[cfg(test)]
    pub(crate) fn with_stop_held(mut self, told: std::sync::mpsc::Sender<()>) -> Self {
        self.stop_hold = Some(StopHold { told, running: 0 });
        self
    }
}

impl Default for FolderProbes {
    fn default() -> Self {
        Self {
            new_folder: Arc::new(probe_folder),
            locate: Arc::new(probe_located),
            placeholder: Arc::new(OsProbe),
            #[cfg(test)]
            stop_hold: None,
        }
    }
}

/// Tests: what [`FolderProbes::with_stop_held`] holds shutdown with.
#[cfg(test)]
#[derive(Clone)]
struct StopHold {
    told: std::sync::mpsc::Sender<()>,
    /// The probes started and not answered on the actor.
    running: usize,
}

/// A probe started (counted only for [`stop_held`]).
pub(in crate::actor) fn begun(actor: &mut Actor) {
    #[cfg(test)]
    if let Some(hold) = &mut actor.folder_probes.stop_hold {
        hold.running += 1;
    }
    let _ = actor;
}

/// A probe's answer reached the actor.
pub(in crate::actor) fn answered(actor: &mut Actor) {
    #[cfg(test)]
    if let Some(hold) = &mut actor.folder_probes.stop_hold {
        hold.running = hold.running.saturating_sub(1);
    }
    let _ = actor;
}

/// Whether shutdown waits for a probe still running: never, but in tests that ask it to
/// ([`FolderProbes::with_stop_held`]), which hear each time it does.
pub(in crate::actor) fn stop_held(actor: &Actor) -> bool {
    #[cfg(test)]
    if let Some(hold) = &actor.folder_probes.stop_hold
        && hold.running > 0
        && matches!(actor.phase, HostPhase::Stopping(_))
    {
        let _gone = hold.told.send(());
        return true;
    }
    let _ = actor;
    false
}

/// Whether `folder` is a folder, and whether `path` lies in it once both resolve
/// (`file_link::inside`); a folder that is not there holds nothing.
pub(crate) fn probe_located(path: &Path, folder: &Path) -> (bool, bool) {
    if !folder.is_dir() {
        return (false, false);
    }
    (true, crate::file_link::inside(path, folder))
}

/// `<parent>/<name>`, its parent as the file system names it (a parent that does not exist,
/// the default location before its first matter, is taken as written), and whether it exists.
pub(crate) fn probe_folder(parent: &Path, name: &str) -> (PathBuf, bool) {
    let parent = dunce::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let folder = parent.join(name);
    let exists = folder.exists();
    (folder, exists)
}

/// A probe's answer for command `command`, with what its create goes on with.
pub(crate) struct Probed {
    command: CommandId,
    name: String,
    details: Details,
    /// The folder and whether it exists, or `off_actor`'s failure (no answer in time).
    found: Result<(PathBuf, bool), Error>,
}

/// Starts the probe of `<parent>/<name>` for command `command`.
pub(in crate::actor) fn start(
    actor: &mut Actor,
    command: CommandId,
    (name, details): (String, Details),
    parent: PathBuf,
) {
    begun(actor);
    let (clock, probe) = (
        Arc::clone(&actor.clock),
        Arc::clone(&actor.folder_probes.new_folder),
    );
    actor.spawn_post(async move {
        let asked = name.clone();
        let found = off_actor(clock, PROBE_JOB, move || Ok(probe(&parent, &asked))).await;
        Input::NewFolder(Box::new(Probed {
            command,
            name,
            details,
            found,
        }))
    });
}

/// The probe answered: the create goes on when its command still waits for it and the host is
/// not stopping.
pub(in crate::actor) fn on_probed(actor: &mut Actor, probed: Probed) {
    answered(actor);
    let Probed {
        command,
        name,
        details,
        found,
    } = probed;
    let waiting = actor
        .commands
        .get(&command)
        .is_some_and(|pending| pending.matter.is_none());
    if !waiting {
        tracing::warn!(command = %command, "a new matter folder check answered for no command");
        return;
    }
    if matches!(actor.phase, HostPhase::Stopping(_)) {
        tracing::info!(command = %command, "the host is stopping: no new matter");
        actor.finish_command(command, Err(Error::Closed));
        return;
    }
    match found {
        Ok((folder, false)) => begin_create(actor, command, (name, details), folder, false),
        Ok((folder, true)) => {
            let exists = Error::refused(
                ErrorCode::CommandInvalid,
                format!("{} already exists; import it instead", folder.display()),
            );
            not_begun(actor, command, exists);
        }
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "a new matter's folder did not answer");
            let refusal = Error::refused(ErrorCode::MatterFolderMissing, NO_ANSWER);
            not_begun(actor, command, refusal);
        }
    }
}

#[cfg(test)]
mod tests;
